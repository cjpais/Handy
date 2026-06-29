use crate::managers::google_api::{CalendarDateTime, CalendarEvent, GoogleApi};
use crate::managers::google_oauth::GoogleOAuth;
use crate::overlay::MeetingOverlayPrompt;
use crate::settings::{get_settings, GoogleFeature};
use crate::TranscriptionCoordinator;
use chrono::{DateTime, Duration, Utc};
use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Manager};

const LOCAL_POLL_SECS: u64 = 1;
const CALENDAR_POLL_SECS: u64 = 60;
const GOOGLE_MEET_PROVIDER: &str = "Google Meet";
const ZOOM_PROVIDER: &str = "Zoom";
const TEAMS_PROVIDER: &str = "Microsoft Teams";

static GOOGLE_MEET_URL_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"(?i)(https?://)?meet\.google\.com/(?P<code>[a-z]{3}-[a-z]{4}-[a-z]{3})(?:[/?#][^\s)]*)?",
    )
    .expect("valid Google Meet regex")
});
static GOOGLE_MEET_TITLE_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)\b[a-z]{3}-[a-z]{4}-[a-z]{3}\b.*google meet|google meet.*\b[a-z]{3}-[a-z]{4}-[a-z]{3}\b")
        .expect("valid Google Meet title regex")
});
static GOOGLE_MEET_IGNORED_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)meet\.google\.com/(landing|new|lookup|_meet/)")
        .expect("valid Google Meet ignored regex")
});
static ZOOM_URL_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)(https?://)?(?:[\w-]+\.)?zoom\.us/(?P<path>(?:j|w|wc)/[A-Za-z0-9?&=/%._-]+)")
        .expect("valid Zoom regex")
});
static ZOOM_IGNORED_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)zoom\.us/(download|launch|profile|signin|signup)")
        .expect("valid Zoom ignored regex")
});
static TEAMS_URL_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)(https?://)?(?P<host>teams\.microsoft\.com/l/meetup-join/[^\s)]+|teams\.live\.com/meet/[^\s)]+)")
        .expect("valid Teams regex")
});

const BROWSER_PROCESS_SUBSTRINGS: &[&str] = &["chrome", "msedge", "firefox", "brave", "opera"];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Type)]
pub enum MeetingPromptSource {
    LocalDetection,
    GoogleCalendar,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Type)]
pub struct MeetingPromptPayload {
    pub provider: String,
    pub title: String,
    pub source: MeetingPromptSource,
    pub start_time: String,
    pub join_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Type, Default)]
pub struct GoogleIntegrationStatus {
    pub oauth_client_configured: bool,
    pub gmail_tasks_connected: bool,
    pub calendar_connected: bool,
    pub gmail_tasks_available: bool,
    pub calendar_available: bool,
    pub meeting_calendar_prompts_enabled: bool,
    pub meeting_detection_enabled: bool,
    pub meeting_prompt_lead_minutes: u32,
}

#[derive(Debug, Clone)]
pub struct MeetingCandidate {
    pub provider: String,
    pub title: String,
    pub source: MeetingPromptSource,
    pub start_time: DateTime<Utc>,
    pub join_url: Option<String>,
}

#[derive(Default)]
struct MeetingAssistantState {
    last_prompted_key: Option<String>,
    suppressed_key: Option<String>,
    active_prompt: Option<MeetingPromptPayload>,
}

pub struct MeetingAssistantManager {
    app: AppHandle,
    state: Arc<Mutex<MeetingAssistantState>>,
}

impl MeetingAssistantManager {
    pub fn new(app: &AppHandle) -> Arc<Self> {
        Arc::new(Self {
            app: app.clone(),
            state: Arc::new(Mutex::new(MeetingAssistantState::default())),
        })
    }

    pub fn start(self: Arc<Self>) {
        let local_self = self.clone();
        tauri::async_runtime::spawn(async move {
            let mut interval =
                tokio::time::interval(tokio::time::Duration::from_secs(LOCAL_POLL_SECS));
            loop {
                interval.tick().await;
                local_self.poll_local_detection().await;
            }
        });

        let calendar_self = self.clone();
        tauri::async_runtime::spawn(async move {
            let mut interval =
                tokio::time::interval(tokio::time::Duration::from_secs(CALENDAR_POLL_SECS));
            loop {
                interval.tick().await;
                calendar_self.poll_calendar().await;
            }
        });
    }

    pub fn google_status(&self) -> GoogleIntegrationStatus {
        let settings = get_settings(&self.app);
        let configured = GoogleOAuth::is_client_id_configured();
        GoogleIntegrationStatus {
            oauth_client_configured: configured,
            gmail_tasks_connected: settings
                .google_auth_tokens
                .gmail_tasks_refresh_token
                .is_some(),
            calendar_connected: settings.google_auth_tokens.calendar_refresh_token.is_some(),
            gmail_tasks_available: configured,
            calendar_available: configured,
            meeting_calendar_prompts_enabled: settings.meeting_calendar_prompts_enabled,
            meeting_detection_enabled: settings.meeting_detection_enabled,
            meeting_prompt_lead_minutes: settings.meeting_prompt_lead_minutes,
        }
    }

    async fn poll_local_detection(&self) {
        let settings = get_settings(&self.app);
        if !settings.meeting_detection_enabled || self.should_suppress_prompts() {
            return;
        }

        if let Some(window) = get_active_window_metadata() {
            if let Some(candidate) = match_active_window(&window) {
                self.try_emit_prompt(candidate);
            }
        }
    }

    async fn poll_calendar(&self) {
        let settings = get_settings(&self.app);
        if !settings.meeting_calendar_prompts_enabled
            || settings.google_auth_tokens.calendar_refresh_token.is_none()
            || self.should_suppress_prompts()
        {
            return;
        }

        let Some(refresh_token) = settings.google_auth_tokens.calendar_refresh_token else {
            return;
        };

        let token = match GoogleOAuth::refresh_token(&refresh_token, GoogleFeature::Calendar).await
        {
            Ok(token) => token,
            Err(err) => {
                log::warn!("Failed to refresh Google Calendar token: {err}");
                return;
            }
        };

        let now = Utc::now();
        let lead = Duration::minutes(settings.meeting_prompt_lead_minutes as i64);
        let time_max = now + lead + Duration::minutes(2);
        let events = match GoogleApi::list_upcoming_events(&token.access_token, now, time_max).await
        {
            Ok(events) => events,
            Err(err) => {
                log::warn!("Failed to poll Google Calendar events: {err}");
                return;
            }
        };

        for event in events {
            if let Some(candidate) =
                calendar_event_to_candidate(&event, settings.meeting_prompt_lead_minutes)
            {
                self.try_emit_prompt(candidate);
            }
        }
    }

    fn should_suppress_prompts(&self) -> bool {
        if let Some(coordinator) = self.app.try_state::<TranscriptionCoordinator>() {
            coordinator.current_mode() != "idle"
        } else {
            false
        }
    }

    fn try_emit_prompt(&self, candidate: MeetingCandidate) {
        let key = prompt_key(&candidate);
        let payload = MeetingPromptPayload {
            provider: candidate.provider,
            title: candidate.title,
            source: candidate.source,
            start_time: candidate.start_time.to_rfc3339(),
            join_url: candidate.join_url,
        };

        {
            let mut state = self.state.lock().expect("meeting assistant state poisoned");
            if state.suppressed_key.as_deref() == Some(&key) {
                return;
            }
            if state.last_prompted_key.as_deref() == Some(&key) {
                return;
            }
            state.last_prompted_key = Some(key);
            state.active_prompt = Some(payload.clone());
        }

        crate::overlay::show_meeting_suggestion_overlay(
            &self.app,
            MeetingOverlayPrompt {
                provider: payload.provider,
                title: payload.title,
                source: payload.source,
                start_time: payload.start_time,
                join_url: payload.join_url,
            },
        );
    }

    pub fn dismiss_prompt(&self, payload: &MeetingPromptPayload) {
        self.suppress_prompt(payload);
    }

    pub fn suppress_prompt(&self, payload: &MeetingPromptPayload) {
        let candidate = MeetingCandidate {
            provider: payload.provider.clone(),
            title: payload.title.clone(),
            source: payload.source.clone(),
            start_time: DateTime::parse_from_rfc3339(&payload.start_time)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now()),
            join_url: payload.join_url.clone(),
        };
        let key = prompt_key(&candidate);
        let mut state = self.state.lock().expect("meeting assistant state poisoned");
        state.suppressed_key = Some(key);
        state.last_prompted_key = None;
        state.active_prompt = None;
    }

    pub fn suppress_active_prompt(&self) {
        let payload = self
            .state
            .lock()
            .expect("meeting assistant state poisoned")
            .active_prompt
            .clone();
        if let Some(payload) = payload {
            self.suppress_prompt(&payload);
        }
    }

    pub fn clear_active_prompt(&self) {
        let mut state = self.state.lock().expect("meeting assistant state poisoned");
        state.last_prompted_key = None;
        state.active_prompt = None;
    }
}

fn prompt_key(candidate: &MeetingCandidate) -> String {
    let meeting_identity = candidate
        .join_url
        .clone()
        .unwrap_or_else(|| candidate.title.trim().to_ascii_lowercase());

    match candidate.source {
        MeetingPromptSource::LocalDetection => {
            format!("local|{}|{}", candidate.provider, meeting_identity)
        }
        MeetingPromptSource::GoogleCalendar => format!(
            "calendar|{}|{}|{}",
            candidate.provider,
            candidate.start_time.to_rfc3339(),
            meeting_identity
        ),
    }
}

#[derive(Debug, Clone)]
struct ActiveWindowMetadata {
    title: String,
    process_name: String,
    accessible_text: Option<String>,
}

fn match_active_window(window: &ActiveWindowMetadata) -> Option<MeetingCandidate> {
    let title = window.title.to_lowercase();
    let process = window.process_name.to_lowercase();
    let accessible = window
        .accessible_text
        .as_deref()
        .unwrap_or_default()
        .to_lowercase();
    let combined = if accessible.is_empty() {
        title.clone()
    } else {
        format!("{title}\n{accessible}")
    };

    let detected = if let Some(url) = detect_google_meet_url(&combined) {
        Some((GOOGLE_MEET_PROVIDER, Some(url)))
    } else if let Some(url) = detect_teams_url(&combined) {
        Some((TEAMS_PROVIDER, Some(url)))
    } else if let Some(url) = detect_zoom_url(&combined) {
        Some((ZOOM_PROVIDER, Some(url)))
    } else if is_browser_process(&process) && GOOGLE_MEET_TITLE_RE.is_match(&title) {
        Some((GOOGLE_MEET_PROVIDER, None))
    } else if process.contains("zoom")
        && ["zoom meeting", "meeting controls", "join meeting"]
            .iter()
            .any(|needle| title.contains(needle))
    {
        Some((ZOOM_PROVIDER, None))
    } else if process.contains("teams")
        && ["meeting", "call", "meet now", "pre-join"]
            .iter()
            .any(|needle| title.contains(needle))
    {
        Some((TEAMS_PROVIDER, None))
    } else {
        None
    };

    if let Some((provider, join_url)) = detected {
        return Some(MeetingCandidate {
            provider: provider.to_string(),
            title: window.title.clone(),
            source: MeetingPromptSource::LocalDetection,
            start_time: Utc::now(),
            join_url,
        });
    }

    None
}

fn calendar_event_to_candidate(
    event: &CalendarEvent,
    lead_minutes: u32,
) -> Option<MeetingCandidate> {
    let start_time = parse_calendar_datetime(&event.start)?;
    let now = Utc::now();
    let lead = Duration::minutes(lead_minutes as i64);
    if start_time < now || start_time > now + lead {
        return None;
    }

    let join_url = extract_meeting_url_from_event(event)?;
    let provider = provider_for_url(&join_url)?;

    Some(MeetingCandidate {
        provider,
        title: event
            .summary
            .clone()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| "Upcoming meeting".to_string()),
        source: MeetingPromptSource::GoogleCalendar,
        start_time,
        join_url: Some(join_url),
    })
}

fn extract_meeting_url_from_event(event: &CalendarEvent) -> Option<String> {
    if let Some(link) = event.hangout_link.clone() {
        if provider_for_url(&link).is_some() {
            return Some(link);
        }
    }
    if let Some(conference_data) = &event.conference_data {
        for entry in &conference_data.entry_points {
            if let Some(uri) = &entry.uri {
                if provider_for_url(uri).is_some() {
                    return Some(uri.clone());
                }
            }
        }
    }
    for field in [&event.location, &event.description] {
        if let Some(text) = field {
            if let Some(url) = extract_url(text) {
                if provider_for_url(&url).is_some() {
                    return Some(url);
                }
            }
        }
    }
    None
}

fn provider_for_url(url: &str) -> Option<String> {
    if detect_google_meet_url(url).is_some() {
        Some(GOOGLE_MEET_PROVIDER.to_string())
    } else if detect_zoom_url(url).is_some() {
        Some(ZOOM_PROVIDER.to_string())
    } else if detect_teams_url(url).is_some() {
        Some(TEAMS_PROVIDER.to_string())
    } else {
        None
    }
}

fn parse_calendar_datetime(value: &CalendarDateTime) -> Option<DateTime<Utc>> {
    if let Some(date_time) = &value.date_time {
        DateTime::parse_from_rfc3339(date_time)
            .ok()
            .map(|dt| dt.with_timezone(&Utc))
    } else if let Some(date) = &value.date {
        DateTime::parse_from_rfc3339(&format!("{date}T00:00:00Z"))
            .ok()
            .map(|dt| dt.with_timezone(&Utc))
    } else {
        None
    }
}

fn extract_url(text: &str) -> Option<String> {
    let regex = Regex::new(r"https?://[^\s)]+").ok()?;
    regex.find(text).map(|m| m.as_str().to_string())
}

fn is_browser_process(process: &str) -> bool {
    BROWSER_PROCESS_SUBSTRINGS
        .iter()
        .any(|needle| process.contains(needle))
}

fn normalize_url(url: &str) -> String {
    if url.to_lowercase().starts_with("http://") || url.to_lowercase().starts_with("https://") {
        url.to_string()
    } else {
        format!("https://{url}")
    }
}

fn detect_google_meet_url(text: &str) -> Option<String> {
    if GOOGLE_MEET_IGNORED_RE.is_match(text) {
        return None;
    }

    GOOGLE_MEET_URL_RE
        .captures(text)
        .and_then(|captures| captures.get(0))
        .map(|m| normalize_url(m.as_str()))
}

fn detect_zoom_url(text: &str) -> Option<String> {
    if ZOOM_IGNORED_RE.is_match(text) {
        return None;
    }

    ZOOM_URL_RE
        .captures(text)
        .and_then(|captures| captures.get(0))
        .map(|m| normalize_url(m.as_str()))
}

fn detect_teams_url(text: &str) -> Option<String> {
    TEAMS_URL_RE
        .captures(text)
        .and_then(|captures| captures.get(0))
        .map(|m| normalize_url(m.as_str()))
}

#[cfg(target_os = "windows")]
fn get_active_window_metadata() -> Option<ActiveWindowMetadata> {
    use std::collections::BTreeSet;
    use windows::core::PWSTR;
    use windows::Win32::Foundation::RPC_E_CHANGED_MODE;
    use windows::Win32::Foundation::{CloseHandle, HWND, MAX_PATH};
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER,
        COINIT_APARTMENTTHREADED,
    };
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_FORMAT,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows::Win32::System::Variant::{VariantClear, VT_BSTR};
    use windows::Win32::UI::Accessibility::{
        CUIAutomation, IUIAutomation, IUIAutomationElement, TreeScope_Subtree,
        UIA_ValueValuePropertyId,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId,
    };

    unsafe fn add_element_text(element: &IUIAutomationElement, values: &mut BTreeSet<String>) {
        for text in [
            element.CurrentName().ok().map(|s| s.to_string()),
            element.CurrentHelpText().ok().map(|s| s.to_string()),
            element.CurrentItemType().ok().map(|s| s.to_string()),
            element
                .CurrentLocalizedControlType()
                .ok()
                .map(|s| s.to_string()),
            element.CurrentAutomationId().ok().map(|s| s.to_string()),
            element.CurrentClassName().ok().map(|s| s.to_string()),
        ] {
            if let Some(text) = text {
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    values.insert(trimmed.to_string());
                }
            }
        }

        if let Ok(mut variant) = element.GetCurrentPropertyValue(UIA_ValueValuePropertyId) {
            let text = if variant.Anonymous.Anonymous.vt == VT_BSTR {
                let value = variant.Anonymous.Anonymous.Anonymous.bstrVal.clone();
                Some(value.to_string())
            } else {
                None
            };
            let _ = VariantClear(&mut variant);

            if let Some(text) = text {
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    values.insert(trimmed.to_string());
                }
            }
        }
    }

    unsafe fn get_accessible_text(hwnd: HWND) -> Option<String> {
        let init = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let should_uninit = init.is_ok();
        if !init.is_ok() && init != RPC_E_CHANGED_MODE {
            return None;
        }

        let result = (|| {
            let automation: IUIAutomation =
                CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).ok()?;
            let root = automation.ElementFromHandle(hwnd).ok()?;
            let condition = automation.CreateTrueCondition().ok()?;
            let elements = root.FindAll(TreeScope_Subtree, &condition).ok()?;
            let len = elements.Length().ok()?.min(96);
            let mut values = BTreeSet::new();

            for index in 0..len {
                let element = elements.GetElement(index).ok()?;
                add_element_text(&element, &mut values);
            }

            if values.is_empty() {
                None
            } else {
                Some(values.into_iter().collect::<Vec<_>>().join("\n"))
            }
        })();

        if should_uninit {
            CoUninitialize();
        }

        result
    }

    unsafe {
        let hwnd: HWND = GetForegroundWindow();
        if hwnd.0.is_null() {
            return None;
        }

        let title_len = GetWindowTextLengthW(hwnd);
        if title_len <= 0 {
            return None;
        }

        let mut title_buf = vec![0u16; title_len as usize + 1];
        let copied = GetWindowTextW(hwnd, &mut title_buf);
        let title = String::from_utf16_lossy(&title_buf[..copied as usize]);

        let mut process_id = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut process_id));
        if process_id == 0 {
            return Some(ActiveWindowMetadata {
                title,
                process_name: String::new(),
                accessible_text: None,
            });
        }

        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process_id).ok()?;
        let mut size = MAX_PATH;
        let mut buf = vec![0u16; MAX_PATH as usize];
        let process_name = if QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_FORMAT(0),
            PWSTR(buf.as_mut_ptr()),
            &mut size,
        )
        .is_ok()
        {
            let path = String::from_utf16_lossy(&buf[..size as usize]);
            std::path::Path::new(&path)
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or(path)
        } else {
            String::new()
        };
        let _ = CloseHandle(handle);
        let is_browser = is_browser_process(&process_name.to_lowercase());

        Some(ActiveWindowMetadata {
            title,
            process_name,
            accessible_text: if is_browser {
                get_accessible_text(hwnd)
            } else {
                None
            },
        })
    }
}

#[cfg(not(target_os = "windows"))]
fn get_active_window_metadata() -> Option<ActiveWindowMetadata> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn provider_matching_detects_zoom_window() {
        let candidate = match_active_window(&ActiveWindowMetadata {
            title: "Zoom Meeting - Weekly Sync".to_string(),
            process_name: "Zoom".to_string(),
            accessible_text: None,
        })
        .expect("expected zoom match");
        assert_eq!(candidate.provider, ZOOM_PROVIDER);
    }

    #[test]
    fn provider_matching_detects_google_meet_from_browser_accessible_url() {
        let candidate = match_active_window(&ActiveWindowMetadata {
            title: "Sprint Review - Google Chrome".to_string(),
            process_name: "chrome".to_string(),
            accessible_text: Some(
                "Address and search bar\nhttps://meet.google.com/abc-defg-hij".to_string(),
            ),
        })
        .expect("expected Google Meet match");
        assert_eq!(candidate.provider, GOOGLE_MEET_PROVIDER);
        assert_eq!(
            candidate.join_url.as_deref(),
            Some("https://meet.google.com/abc-defg-hij")
        );
    }

    #[test]
    fn provider_matching_ignores_google_meet_launcher_page() {
        let candidate = match_active_window(&ActiveWindowMetadata {
            title: "Google Meet - Google Chrome".to_string(),
            process_name: "chrome".to_string(),
            accessible_text: Some("https://meet.google.com/landing".to_string()),
        });
        assert!(candidate.is_none());
    }

    #[test]
    fn provider_matching_detects_teams_browser_url() {
        let candidate = match_active_window(&ActiveWindowMetadata {
            title: "Project sync | Microsoft Teams".to_string(),
            process_name: "msedge".to_string(),
            accessible_text: Some(
                "https://teams.microsoft.com/l/meetup-join/19%3ameeting_id%40thread.v2/0"
                    .to_string(),
            ),
        })
        .expect("expected Teams match");
        assert_eq!(candidate.provider, TEAMS_PROVIDER);
    }

    #[test]
    fn calendar_event_parsing_finds_meet_link() {
        let event: CalendarEvent = serde_json::from_value(json!({
            "summary": "Standup",
            "start": { "dateTime": (Utc::now() + Duration::minutes(3)).to_rfc3339() },
            "hangoutLink": "https://meet.google.com/abc-defg-hij"
        }))
        .expect("valid event json");

        let candidate = calendar_event_to_candidate(&event, 5).expect("event should prompt");
        assert_eq!(candidate.provider, GOOGLE_MEET_PROVIDER);
    }

    #[test]
    fn calendar_event_parsing_ignores_non_meeting_google_meet_link() {
        let event: CalendarEvent = serde_json::from_value(json!({
            "summary": "Standup",
            "start": { "dateTime": (Utc::now() + Duration::minutes(3)).to_rfc3339() },
            "description": "Join here: https://meet.google.com/landing"
        }))
        .expect("valid event json");

        assert!(calendar_event_to_candidate(&event, 5).is_none());
    }

    #[test]
    fn scheduling_respects_five_minute_window() {
        let soon: CalendarEvent = serde_json::from_value(json!({
            "summary": "Soon",
            "start": { "dateTime": (Utc::now() + Duration::minutes(4)).to_rfc3339() },
            "hangoutLink": "https://meet.google.com/abc-defg-hij"
        }))
        .unwrap();
        assert!(calendar_event_to_candidate(&soon, 5).is_some());

        let later: CalendarEvent = serde_json::from_value(json!({
            "summary": "Later",
            "start": { "dateTime": (Utc::now() + Duration::minutes(8)).to_rfc3339() },
            "hangoutLink": "https://meet.google.com/abc-defg-hij"
        }))
        .unwrap();
        assert!(calendar_event_to_candidate(&later, 5).is_none());
    }

    #[test]
    fn duplicate_suppression_key_is_stable() {
        let candidate = MeetingCandidate {
            provider: "Google Meet".to_string(),
            title: "Standup".to_string(),
            source: MeetingPromptSource::GoogleCalendar,
            start_time: Utc::now(),
            join_url: Some("https://meet.google.com/abc".to_string()),
        };
        assert_eq!(prompt_key(&candidate), prompt_key(&candidate));
    }

    #[test]
    fn local_detection_key_ignores_poll_timestamp() {
        let first = MeetingCandidate {
            provider: "Zoom".to_string(),
            title: "Zoom Meeting - Weekly Sync".to_string(),
            source: MeetingPromptSource::LocalDetection,
            start_time: Utc::now(),
            join_url: None,
        };
        let second = MeetingCandidate {
            start_time: Utc::now() + Duration::seconds(5),
            ..first.clone()
        };

        assert_eq!(prompt_key(&first), prompt_key(&second));
    }
}

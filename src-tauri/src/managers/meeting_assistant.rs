use crate::managers::google_api::{CalendarDateTime, CalendarEvent, GoogleApi};
use crate::managers::google_oauth::GoogleOAuth;
use crate::settings::{get_settings, GoogleFeature};
use crate::TranscriptionCoordinator;
use chrono::{DateTime, Duration, Utc};
use regex::Regex;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager};

const LOCAL_POLL_SECS: u64 = 4;
const CALENDAR_POLL_SECS: u64 = 60;
const DUPLICATE_COOLDOWN_MINUTES: i64 = 15;

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

#[derive(Debug, Clone)]
struct ProviderMatcher {
    provider: &'static str,
    process_substrings: &'static [&'static str],
    title_substrings: &'static [&'static str],
    url_patterns: &'static [&'static str],
}

const PROVIDER_MATCHERS: &[ProviderMatcher] = &[
    ProviderMatcher {
        provider: "Google Meet",
        process_substrings: &["chrome", "msedge", "firefox", "brave"],
        title_substrings: &["google meet", "meet.google.com"],
        url_patterns: &["meet.google.com/"],
    },
    ProviderMatcher {
        provider: "Zoom",
        process_substrings: &["zoom"],
        title_substrings: &["zoom meeting", "zoom workplace"],
        url_patterns: &["zoom.us/j/", "zoom.us/wc/"],
    },
    ProviderMatcher {
        provider: "Microsoft Teams",
        process_substrings: &["teams"],
        title_substrings: &["microsoft teams", "teams meeting", "teams |"],
        url_patterns: &["teams.microsoft.com/l/meetup-join", "teams.live.com/meet/"],
    },
    ProviderMatcher {
        provider: "Webex",
        process_substrings: &["webex"],
        title_substrings: &["webex"],
        url_patterns: &["webex.com/meet", "webex.com/join"],
    },
    ProviderMatcher {
        provider: "Jitsi",
        process_substrings: &["chrome", "msedge", "firefox"],
        title_substrings: &["jitsi meet"],
        url_patterns: &["meet.jit.si/"],
    },
    ProviderMatcher {
        provider: "Whereby",
        process_substrings: &["chrome", "msedge", "firefox"],
        title_substrings: &["whereby"],
        url_patterns: &["whereby.com/"],
    },
    ProviderMatcher {
        provider: "Slack Huddle",
        process_substrings: &["slack"],
        title_substrings: &["huddle", "slack call", "slack |"],
        url_patterns: &["app.slack.com/huddle", "slack.com/calls/"],
    },
    ProviderMatcher {
        provider: "Discord",
        process_substrings: &["discord"],
        title_substrings: &["discord", "voice call", "video call"],
        url_patterns: &["discord.com/channels/"],
    },
];

#[derive(Default)]
struct MeetingAssistantState {
    dismissed_until: HashMap<String, DateTime<Utc>>,
    last_prompted_key: Option<String>,
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
        let now = Utc::now();
        {
            let mut state = self.state.lock().expect("meeting assistant state poisoned");
            if let Some(until) = state.dismissed_until.get(&key) {
                if *until > now {
                    return;
                }
            }
            if state.last_prompted_key.as_deref() == Some(&key) {
                return;
            }
            state.last_prompted_key = Some(key);
        }

        crate::overlay::show_meeting_prompt_window(&self.app);
        let _ = self.app.emit(
            "meeting-prompt-show",
            MeetingPromptPayload {
                provider: candidate.provider,
                title: candidate.title,
                source: candidate.source,
                start_time: candidate.start_time.to_rfc3339(),
                join_url: candidate.join_url,
            },
        );
    }

    pub fn dismiss_prompt(&self, payload: &MeetingPromptPayload) {
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
        state.dismissed_until.insert(
            key,
            Utc::now() + Duration::minutes(DUPLICATE_COOLDOWN_MINUTES),
        );
        state.last_prompted_key = None;
    }

    pub fn clear_active_prompt(&self) {
        let mut state = self.state.lock().expect("meeting assistant state poisoned");
        state.last_prompted_key = None;
    }
}

fn prompt_key(candidate: &MeetingCandidate) -> String {
    format!(
        "{}|{}|{}",
        candidate.provider,
        candidate.start_time.to_rfc3339(),
        candidate.join_url.clone().unwrap_or_default()
    )
}

#[derive(Debug, Clone)]
struct ActiveWindowMetadata {
    title: String,
    process_name: String,
}

fn match_active_window(window: &ActiveWindowMetadata) -> Option<MeetingCandidate> {
    let title = window.title.to_lowercase();
    let process = window.process_name.to_lowercase();

    for matcher in PROVIDER_MATCHERS {
        let process_match = matcher
            .process_substrings
            .iter()
            .any(|needle| process.contains(needle));
        let title_match = matcher
            .title_substrings
            .iter()
            .any(|needle| title.contains(needle));
        let url_match = matcher
            .url_patterns
            .iter()
            .any(|needle| title.contains(needle));
        if process_match && (title_match || url_match) || url_match {
            return Some(MeetingCandidate {
                provider: matcher.provider.to_string(),
                title: window.title.clone(),
                source: MeetingPromptSource::LocalDetection,
                start_time: Utc::now(),
                join_url: extract_url(&window.title),
            });
        }
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
        return Some(link);
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
    let lower = url.to_lowercase();
    PROVIDER_MATCHERS
        .iter()
        .find(|matcher| {
            matcher
                .url_patterns
                .iter()
                .any(|pattern| lower.contains(pattern))
        })
        .map(|matcher| matcher.provider.to_string())
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

#[cfg(target_os = "windows")]
fn get_active_window_metadata() -> Option<ActiveWindowMetadata> {
    use windows::core::PWSTR;
    use windows::Win32::Foundation::{CloseHandle, HWND, MAX_PATH};
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_FORMAT,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId,
    };

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

        Some(ActiveWindowMetadata {
            title,
            process_name,
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
        })
        .expect("expected zoom match");
        assert_eq!(candidate.provider, "Zoom");
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
        assert_eq!(candidate.provider, "Google Meet");
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
}

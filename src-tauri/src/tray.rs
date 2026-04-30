use crate::managers::history::{HistoryEntry, HistoryManager};
use crate::managers::model::ModelManager;
use crate::managers::transcription::TranscriptionManager;
use crate::settings;
use crate::tray_i18n::get_tray_translations;
use log::{error, info, warn};
use std::sync::Arc;
use tauri::image::Image;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::TrayIcon;
use tauri::{AppHandle, Manager, Theme};
use tauri_plugin_clipboard_manager::ClipboardExt;

#[derive(Clone, Debug, PartialEq)]
pub enum TrayIconState {
    Idle,
    Recording,
    Transcribing,
}

#[derive(Clone, Debug, PartialEq)]
pub enum AppTheme {
    Dark,
    Light,
    Colored, // Pink/colored theme for Linux
}

/// Gets the current app theme, with Linux defaulting to Colored theme
pub fn get_current_theme(app: &AppHandle) -> AppTheme {
    if cfg!(target_os = "linux") {
        // On Linux, always use the colored theme
        AppTheme::Colored
    } else {
        // On other platforms, map system theme to our app theme
        if let Some(main_window) = app.get_webview_window("main") {
            match main_window.theme().unwrap_or(Theme::Dark) {
                Theme::Light => AppTheme::Light,
                Theme::Dark => AppTheme::Dark,
                _ => AppTheme::Dark, // Default fallback
            }
        } else {
            AppTheme::Dark
        }
    }
}

/// Gets the appropriate icon path for the given theme and state
pub fn get_icon_path(theme: AppTheme, state: TrayIconState) -> &'static str {
    match (theme, state) {
        // Dark theme uses light icons
        (AppTheme::Dark, TrayIconState::Idle) => "resources/tray_idle.png",
        (AppTheme::Dark, TrayIconState::Recording) => "resources/tray_recording.png",
        (AppTheme::Dark, TrayIconState::Transcribing) => "resources/tray_transcribing.png",
        // Light theme uses dark icons
        (AppTheme::Light, TrayIconState::Idle) => "resources/tray_idle_dark.png",
        (AppTheme::Light, TrayIconState::Recording) => "resources/tray_recording_dark.png",
        (AppTheme::Light, TrayIconState::Transcribing) => "resources/tray_transcribing_dark.png",
        // Colored theme uses pink icons (for Linux)
        (AppTheme::Colored, TrayIconState::Idle) => "resources/handy.png",
        (AppTheme::Colored, TrayIconState::Recording) => "resources/recording.png",
        (AppTheme::Colored, TrayIconState::Transcribing) => "resources/transcribing.png",
    }
}

pub fn change_tray_icon(app: &AppHandle, icon: TrayIconState) {
    let tray = app.state::<TrayIcon>();
    let theme = get_current_theme(app);

    let icon_path = get_icon_path(theme, icon.clone());

    let _ = tray.set_icon(Some(
        Image::from_path(
            app.path()
                .resolve(icon_path, tauri::path::BaseDirectory::Resource)
                .expect("failed to resolve"),
        )
        .expect("failed to set icon"),
    ));

    // Update menu based on state
    update_tray_menu(app, &icon, None);
}

pub fn tray_tooltip() -> String {
    version_label()
}

fn version_label() -> String {
    if cfg!(debug_assertions) {
        format!("Handy v{} (Dev)", env!("CARGO_PKG_VERSION"))
    } else {
        format!("Handy v{}", env!("CARGO_PKG_VERSION"))
    }
}

/// Human-readable name for a language code (matches frontend labels).
fn language_name(code: &str) -> &str {
    match code {
        "auto" => "Auto Detect",
        "en" => "English",
        "zh-Hans" => "Simplified Chinese",
        "zh-Hant" => "Traditional Chinese",
        "yue" => "Cantonese",
        "de" => "German",
        "es" => "Spanish",
        "ru" => "Russian",
        "ko" => "Korean",
        "fr" => "French",
        "ja" => "Japanese",
        "pt" => "Portuguese",
        "tr" => "Turkish",
        "pl" => "Polish",
        "ca" => "Catalan",
        "nl" => "Dutch",
        "ar" => "Arabic",
        "sv" => "Swedish",
        "it" => "Italian",
        "id" => "Indonesian",
        "hi" => "Hindi",
        "fi" => "Finnish",
        "vi" => "Vietnamese",
        "he" => "Hebrew",
        "uk" => "Ukrainian",
        "el" => "Greek",
        "ms" => "Malay",
        "cs" => "Czech",
        "ro" => "Romanian",
        "da" => "Danish",
        "hu" => "Hungarian",
        "ta" => "Tamil",
        "no" => "Norwegian",
        "th" => "Thai",
        "ur" => "Urdu",
        "hr" => "Croatian",
        "bg" => "Bulgarian",
        "lt" => "Lithuanian",
        "la" => "Latin",
        "mi" => "Maori",
        "ml" => "Malayalam",
        "cy" => "Welsh",
        "sk" => "Slovak",
        "te" => "Telugu",
        "fa" => "Persian",
        "lv" => "Latvian",
        "bn" => "Bengali",
        "sr" => "Serbian",
        "az" => "Azerbaijani",
        "sl" => "Slovenian",
        "kn" => "Kannada",
        "et" => "Estonian",
        "mk" => "Macedonian",
        "br" => "Breton",
        "eu" => "Basque",
        "is" => "Icelandic",
        "hy" => "Armenian",
        "ne" => "Nepali",
        "mn" => "Mongolian",
        "bs" => "Bosnian",
        "kk" => "Kazakh",
        "sq" => "Albanian",
        "sw" => "Swahili",
        "gl" => "Galician",
        "mr" => "Marathi",
        "pa" => "Punjabi",
        "si" => "Sinhala",
        "km" => "Khmer",
        "sn" => "Shona",
        "yo" => "Yoruba",
        "so" => "Somali",
        "af" => "Afrikaans",
        "oc" => "Occitan",
        "ka" => "Georgian",
        "be" => "Belarusian",
        "tg" => "Tajik",
        "sd" => "Sindhi",
        "gu" => "Gujarati",
        "am" => "Amharic",
        "yi" => "Yiddish",
        "lo" => "Lao",
        "uz" => "Uzbek",
        "fo" => "Faroese",
        "ht" => "Haitian Creole",
        "ps" => "Pashto",
        "tk" => "Turkmen",
        "nn" => "Nynorsk",
        "mt" => "Maltese",
        "sa" => "Sanskrit",
        "lb" => "Luxembourgish",
        "my" => "Myanmar",
        "bo" => "Tibetan",
        "tl" => "Tagalog",
        "mg" => "Malagasy",
        "as" => "Assamese",
        "tt" => "Tatar",
        "haw" => "Hawaiian",
        "ln" => "Lingala",
        "ha" => "Hausa",
        "ba" => "Bashkir",
        "jw" => "Javanese",
        "su" => "Sundanese",
        _ => code,
    }
}

/// Build the language submenu for the tray menu.
fn build_language_submenu(
    app: &AppHandle,
    settings: &settings::AppSettings,
    model_manager: &ModelManager,
    strings: &crate::tray_i18n::TrayStrings,
) -> Submenu<tauri::Wry> {
    let current_model_id = &settings.selected_model;
    let current_language = &settings.selected_language;
    let model_info = model_manager.get_model_info(current_model_id);

    let supports_selection = model_info
        .as_ref()
        .map(|m| m.supports_language_selection)
        .unwrap_or(true);
    let supported_languages: Vec<String> = model_info
        .as_ref()
        .map(|m| m.supported_languages.clone())
        .unwrap_or_default();

    // Determine which languages to show
    let languages: Vec<String> = if supported_languages.is_empty() {
        // Model claims all languages (e.g. Whisper)
        vec![
            "auto".to_string(),
            "en".to_string(),
            "zh-Hans".to_string(),
            "zh-Hant".to_string(),
            "yue".to_string(),
            "de".to_string(),
            "es".to_string(),
            "ru".to_string(),
            "ko".to_string(),
            "fr".to_string(),
            "ja".to_string(),
            "pt".to_string(),
            "tr".to_string(),
            "pl".to_string(),
            "ca".to_string(),
            "nl".to_string(),
            "ar".to_string(),
            "sv".to_string(),
            "it".to_string(),
            "id".to_string(),
            "hi".to_string(),
            "fi".to_string(),
            "vi".to_string(),
            "he".to_string(),
            "uk".to_string(),
            "el".to_string(),
            "ms".to_string(),
            "cs".to_string(),
            "ro".to_string(),
            "da".to_string(),
            "hu".to_string(),
            "ta".to_string(),
            "no".to_string(),
            "th".to_string(),
            "ur".to_string(),
            "hr".to_string(),
            "bg".to_string(),
            "lt".to_string(),
            "la".to_string(),
            "mi".to_string(),
            "ml".to_string(),
            "cy".to_string(),
            "sk".to_string(),
            "te".to_string(),
            "fa".to_string(),
            "lv".to_string(),
            "bn".to_string(),
            "sr".to_string(),
            "az".to_string(),
            "sl".to_string(),
            "kn".to_string(),
            "et".to_string(),
            "mk".to_string(),
            "br".to_string(),
            "eu".to_string(),
            "is".to_string(),
            "hy".to_string(),
            "ne".to_string(),
            "mn".to_string(),
            "bs".to_string(),
            "kk".to_string(),
            "sq".to_string(),
            "sw".to_string(),
            "gl".to_string(),
            "mr".to_string(),
            "pa".to_string(),
            "si".to_string(),
            "km".to_string(),
            "sn".to_string(),
            "yo".to_string(),
            "so".to_string(),
            "af".to_string(),
            "oc".to_string(),
            "ka".to_string(),
            "be".to_string(),
            "tg".to_string(),
            "sd".to_string(),
            "gu".to_string(),
            "am".to_string(),
            "yi".to_string(),
            "lo".to_string(),
            "uz".to_string(),
            "fo".to_string(),
            "ht".to_string(),
            "ps".to_string(),
            "tk".to_string(),
            "nn".to_string(),
            "mt".to_string(),
            "sa".to_string(),
            "lb".to_string(),
            "my".to_string(),
            "bo".to_string(),
            "tl".to_string(),
            "mg".to_string(),
            "as".to_string(),
            "tt".to_string(),
            "haw".to_string(),
            "ln".to_string(),
            "ha".to_string(),
            "ba".to_string(),
            "jw".to_string(),
            "su".to_string(),
        ]
    } else {
        // Start with auto, then add supported languages
        let mut langs = vec!["auto".to_string()];
        for l in &supported_languages {
            if l != "auto" {
                langs.push(l.clone());
            }
        }
        langs
    };

    let language_label = if !supports_selection {
        format!("{} ({})", strings.language, "Auto")
    } else {
        let active_name = language_name(current_language);
        format!("{} ({})", strings.language, active_name)
    };

    let submenu = Submenu::with_id(app, "language_submenu", &language_label, supports_selection)
        .expect("failed to create language submenu");

    if !supports_selection {
        return submenu;
    }

    let show_recent = languages.len() > 10;

    // Add auto detect first
    {
        let is_active = current_language == "auto";
        let item_id = "language_select:auto";
        let item = CheckMenuItem::with_id(
            app,
            item_id,
            language_name("auto"),
            true,
            is_active,
            None::<&str>,
        )
        .expect("failed to create language item");
        let _ = submenu.append(&item);
    }

    // Build recent section if applicable
    let displayed_recent: std::collections::HashSet<String> = if show_recent {
        let supported_set: std::collections::HashSet<_> = languages.iter().cloned().collect();
        let recent: Vec<_> = settings
            .recent_languages
            .iter()
            .filter(|l| supported_set.contains(l.as_str()) && *l != "auto")
            .take(5)
            .cloned()
            .collect();

        if !recent.is_empty() {
            let separator = PredefinedMenuItem::separator(app).expect("failed to create separator");
            let _ = submenu.append(&separator);

            for code in &recent {
                let is_active = current_language == code;
                let item_id = format!("language_select:{}", code);
                let item = CheckMenuItem::with_id(
                    app,
                    &item_id,
                    language_name(code),
                    true,
                    is_active,
                    None::<&str>,
                )
                .expect("failed to create language item");
                let _ = submenu.append(&item);
            }
        }

        recent.into_iter().collect()
    } else {
        std::collections::HashSet::new()
    };

    // Full alphabetical list
    let mut sorted_languages: Vec<_> = languages.iter().filter(|l| *l != "auto").collect();
    sorted_languages.sort_by(|a, b| language_name(a).cmp(language_name(b)));

    if !sorted_languages.is_empty() {
        let separator = PredefinedMenuItem::separator(app).expect("failed to create separator");
        let _ = submenu.append(&separator);

        for code in sorted_languages {
            // Skip items already shown in the recent section
            if show_recent && displayed_recent.contains(code) {
                continue;
            }
            let is_active = current_language == code;
            let item_id = format!("language_select:{}", code);
            let item = CheckMenuItem::with_id(
                app,
                &item_id,
                language_name(code),
                true,
                is_active,
                None::<&str>,
            )
            .expect("failed to create language item");
            let _ = submenu.append(&item);
        }
    }

    submenu
}

pub fn update_tray_menu(app: &AppHandle, state: &TrayIconState, locale: Option<&str>) {
    let settings = settings::get_settings(app);

    let locale = locale.unwrap_or(&settings.app_language);
    let strings = get_tray_translations(Some(locale.to_string()));

    // Platform-specific accelerators
    #[cfg(target_os = "macos")]
    let (settings_accelerator, quit_accelerator) = (Some("Cmd+,"), Some("Cmd+Q"));
    #[cfg(not(target_os = "macos"))]
    let (settings_accelerator, quit_accelerator) = (Some("Ctrl+,"), Some("Ctrl+Q"));

    // Create common menu items
    let version_label = version_label();
    let version_i = MenuItem::with_id(app, "version", &version_label, false, None::<&str>)
        .expect("failed to create version item");
    let settings_i = MenuItem::with_id(
        app,
        "settings",
        &strings.settings,
        true,
        settings_accelerator,
    )
    .expect("failed to create settings item");
    let check_updates_i = MenuItem::with_id(
        app,
        "check_updates",
        &strings.check_updates,
        settings.update_checks_enabled,
        None::<&str>,
    )
    .expect("failed to create check updates item");
    let copy_last_transcript_i = MenuItem::with_id(
        app,
        "copy_last_transcript",
        &strings.copy_last_transcript,
        true,
        None::<&str>,
    )
    .expect("failed to create copy last transcript item");
    let model_loaded = app.state::<Arc<TranscriptionManager>>().is_model_loaded();
    let quit_i = MenuItem::with_id(app, "quit", &strings.quit, true, quit_accelerator)
        .expect("failed to create quit item");
    let separator = || PredefinedMenuItem::separator(app).expect("failed to create separator");

    // Build model submenu — label is the active model name
    let model_manager = app.state::<Arc<ModelManager>>();
    let models = model_manager.get_available_models();
    let current_model_id = &settings.selected_model;

    let mut downloaded: Vec<_> = models.into_iter().filter(|m| m.is_downloaded).collect();
    downloaded.sort_by(|a, b| a.name.cmp(&b.name));

    let submenu_label = downloaded
        .iter()
        .find(|m| m.id == *current_model_id)
        .map(|m| m.name.clone())
        .unwrap_or_else(|| strings.model.clone());

    let model_submenu = {
        let submenu = Submenu::with_id(app, "model_submenu", &submenu_label, true)
            .expect("failed to create model submenu");

        for model in &downloaded {
            let is_active = model.id == *current_model_id;
            let item_id = format!("model_select:{}", model.id);
            let item =
                CheckMenuItem::with_id(app, &item_id, &model.name, true, is_active, None::<&str>)
                    .expect("failed to create model item");
            let _ = submenu.append(&item);
        }

        submenu
    };

    let language_submenu = build_language_submenu(app, &settings, &model_manager, &strings);

    let unload_model_i = MenuItem::with_id(
        app,
        "unload_model",
        &strings.unload_model,
        model_loaded,
        None::<&str>,
    )
    .expect("failed to create unload model item");

    let menu = match state {
        TrayIconState::Recording | TrayIconState::Transcribing => {
            let cancel_i = MenuItem::with_id(app, "cancel", &strings.cancel, true, None::<&str>)
                .expect("failed to create cancel item");
            Menu::with_items(
                app,
                &[
                    &version_i,
                    &separator(),
                    &cancel_i,
                    &separator(),
                    &copy_last_transcript_i,
                    &separator(),
                    &settings_i,
                    &check_updates_i,
                    &separator(),
                    &quit_i,
                ],
            )
            .expect("failed to create menu")
        }
        TrayIconState::Idle => Menu::with_items(
            app,
            &[
                &version_i,
                &separator(),
                &copy_last_transcript_i,
                &separator(),
                &model_submenu,
                &language_submenu,
                &unload_model_i,
                &separator(),
                &settings_i,
                &check_updates_i,
                &separator(),
                &quit_i,
            ],
        )
        .expect("failed to create menu"),
    };

    let tray = app.state::<TrayIcon>();
    let _ = tray.set_menu(Some(menu));
    let _ = tray.set_icon_as_template(true);
    let _ = tray.set_tooltip(Some(version_label));
}

fn last_transcript_text(entry: &HistoryEntry) -> &str {
    entry
        .post_processed_text
        .as_deref()
        .unwrap_or(&entry.transcription_text)
}

pub fn set_tray_visibility(app: &AppHandle, visible: bool) {
    let tray = app.state::<TrayIcon>();
    if let Err(e) = tray.set_visible(visible) {
        error!("Failed to set tray visibility: {}", e);
    } else {
        info!("Tray visibility set to: {}", visible);
    }
}

pub fn copy_last_transcript(app: &AppHandle) {
    let history_manager = app.state::<Arc<HistoryManager>>();
    let entry = match history_manager.get_latest_completed_entry() {
        Ok(Some(entry)) => entry,
        Ok(None) => {
            warn!("No completed transcription history entries available for tray copy.");
            return;
        }
        Err(err) => {
            error!(
                "Failed to fetch last completed transcription entry: {}",
                err
            );
            return;
        }
    };

    let text = last_transcript_text(&entry);
    if text.trim().is_empty() {
        warn!("Last completed transcription is empty; skipping tray copy.");
        return;
    }

    if let Err(err) = app.clipboard().write_text(text) {
        error!("Failed to copy last transcript to clipboard: {}", err);
        return;
    }

    info!("Copied last transcript to clipboard via tray.");
}

#[cfg(test)]
mod tests {
    use super::last_transcript_text;
    use crate::managers::history::HistoryEntry;

    fn build_entry(transcription: &str, post_processed: Option<&str>) -> HistoryEntry {
        HistoryEntry {
            id: 1,
            file_name: "handy-1.wav".to_string(),
            timestamp: 0,
            saved: false,
            title: "Recording".to_string(),
            transcription_text: transcription.to_string(),
            post_processed_text: post_processed.map(|text| text.to_string()),
            post_process_prompt: None,
            post_process_requested: false,
        }
    }

    #[test]
    fn uses_post_processed_text_when_available() {
        let entry = build_entry("raw", Some("processed"));
        assert_eq!(last_transcript_text(&entry), "processed");
    }

    #[test]
    fn falls_back_to_raw_transcription() {
        let entry = build_entry("raw", None);
        assert_eq!(last_transcript_text(&entry), "raw");
    }
}

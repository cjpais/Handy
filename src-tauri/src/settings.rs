use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;
use tauri::{App, AppHandle};
use tauri_plugin_store::StoreExt;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ShortcutBinding {
    pub id: String,
    pub name: String,
    pub description: String,
    pub default_binding: String,
    pub current_binding: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum OverlayPosition {
    None,
    Top,
    Bottom,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ModelUnloadTimeout {
    Never,
    Immediately,
    Min2,
    Min5,
    Min10,
    Min15,
    Hour1,
    Sec5, // Debug mode only
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MicrophoneKeepAlive {
    Off,
    Sec5,
    Sec15,
    Sec30,
    Min1,
    Min5,
    Min15,
    Hour1,
    Forever,
}

impl Default for ModelUnloadTimeout {
    fn default() -> Self {
        ModelUnloadTimeout::Never
    }
}

impl ModelUnloadTimeout {
    pub fn to_minutes(self) -> Option<u64> {
        match self {
            ModelUnloadTimeout::Never => None,
            ModelUnloadTimeout::Immediately => Some(0), // Special case for immediate unloading
            ModelUnloadTimeout::Min2 => Some(2),
            ModelUnloadTimeout::Min5 => Some(5),
            ModelUnloadTimeout::Min10 => Some(10),
            ModelUnloadTimeout::Min15 => Some(15),
            ModelUnloadTimeout::Hour1 => Some(60),
            ModelUnloadTimeout::Sec5 => Some(0), // Special case for debug - handled separately
        }
    }

    pub fn to_seconds(self) -> Option<u64> {
        match self {
            ModelUnloadTimeout::Never => None,
            ModelUnloadTimeout::Immediately => Some(0), // Special case for immediate unloading
            ModelUnloadTimeout::Sec5 => Some(5),
            _ => self.to_minutes().map(|m| m * 60),
        }
    }
}

impl MicrophoneKeepAlive {
    pub fn duration(self) -> Option<Duration> {
        match self {
            MicrophoneKeepAlive::Off => Some(Duration::from_secs(0)),
            MicrophoneKeepAlive::Sec5 => Some(Duration::from_secs(5)),
            MicrophoneKeepAlive::Sec15 => Some(Duration::from_secs(15)),
            MicrophoneKeepAlive::Sec30 => Some(Duration::from_secs(30)),
            MicrophoneKeepAlive::Min1 => Some(Duration::from_secs(60)),
            MicrophoneKeepAlive::Min5 => Some(Duration::from_secs(5 * 60)),
            MicrophoneKeepAlive::Min15 => Some(Duration::from_secs(15 * 60)),
            MicrophoneKeepAlive::Hour1 => Some(Duration::from_secs(60 * 60)),
            MicrophoneKeepAlive::Forever => None,
        }
    }

    pub fn is_forever(self) -> bool {
        matches!(self, MicrophoneKeepAlive::Forever)
    }
}

/* still handy for composing the initial JSON in the store ------------- */
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AppSettings {
    pub bindings: HashMap<String, ShortcutBinding>,
    pub push_to_talk: bool,
    pub audio_feedback: bool,
    #[serde(default = "default_start_hidden")]
    pub start_hidden: bool,
    #[serde(default = "default_model")]
    pub selected_model: String,
    #[serde(default = "default_always_on_microphone")]
    pub always_on_microphone: bool,
    #[serde(default = "default_microphone_keep_alive")]
    pub microphone_keep_alive: MicrophoneKeepAlive,
    #[serde(default)]
    pub selected_microphone: Option<String>,
    #[serde(default)]
    pub selected_output_device: Option<String>,
    #[serde(default = "default_translate_to_english")]
    pub translate_to_english: bool,
    #[serde(default = "default_selected_language")]
    pub selected_language: String,
    #[serde(default = "default_overlay_position")]
    pub overlay_position: OverlayPosition,
    #[serde(default = "default_debug_mode")]
    pub debug_mode: bool,
    #[serde(default)]
    pub custom_words: Vec<String>,
    #[serde(default)]
    pub model_unload_timeout: ModelUnloadTimeout,
    #[serde(default = "default_word_correction_threshold")]
    pub word_correction_threshold: f64,
}

impl AppSettings {
    pub fn set_microphone_keep_alive(&mut self, keep_alive: MicrophoneKeepAlive) {
        self.microphone_keep_alive = keep_alive;
        self.always_on_microphone = keep_alive.is_forever();
    }

    pub fn normalize_microphone_fields(&mut self) {
        if self.always_on_microphone && !self.microphone_keep_alive.is_forever() {
            self.microphone_keep_alive = MicrophoneKeepAlive::Forever;
        }

        if !self.always_on_microphone && self.microphone_keep_alive.is_forever() {
            self.microphone_keep_alive = MicrophoneKeepAlive::Off;
        }

        self.always_on_microphone = self.microphone_keep_alive.is_forever();
    }
}

fn default_model() -> String {
    "".to_string()
}

fn default_always_on_microphone() -> bool {
    false
}

fn default_microphone_keep_alive() -> MicrophoneKeepAlive {
    MicrophoneKeepAlive::Off
}

fn default_translate_to_english() -> bool {
    false
}

fn default_start_hidden() -> bool {
    false
}

fn default_selected_language() -> String {
    "auto".to_string()
}

fn default_overlay_position() -> OverlayPosition {
    OverlayPosition::Bottom
}

fn default_debug_mode() -> bool {
    false
}

fn default_word_correction_threshold() -> f64 {
    0.18
}

pub const SETTINGS_STORE_PATH: &str = "settings_store.json";

pub fn get_default_settings() -> AppSettings {
    #[cfg(target_os = "windows")]
    let default_shortcut = "ctrl+space";
    #[cfg(target_os = "macos")]
    let default_shortcut = "option+space";
    #[cfg(target_os = "linux")]
    let default_shortcut = "ctrl+space";
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    let default_shortcut = "alt+space";

    let mut bindings = HashMap::new();
    bindings.insert(
        "transcribe".to_string(),
        ShortcutBinding {
            id: "transcribe".to_string(),
            name: "Transcribe".to_string(),
            description: "Converts your speech into text.".to_string(),
            default_binding: default_shortcut.to_string(),
            current_binding: default_shortcut.to_string(),
        },
    );

    AppSettings {
        bindings,
        push_to_talk: true,
        audio_feedback: false,
        start_hidden: default_start_hidden(),
        selected_model: default_model(),
        always_on_microphone: false,
        microphone_keep_alive: default_microphone_keep_alive(),
        selected_microphone: None,
        selected_output_device: None,
        translate_to_english: default_translate_to_english(),
        selected_language: default_selected_language(),
        overlay_position: default_overlay_position(),
        debug_mode: default_debug_mode(),
        custom_words: Vec::new(),
        model_unload_timeout: ModelUnloadTimeout::Never,
        word_correction_threshold: default_word_correction_threshold(),
    }
}

pub fn load_or_create_app_settings(app: &App) -> AppSettings {
    // Initialize store
    let store = app
        .store(SETTINGS_STORE_PATH)
        .expect("Failed to initialize store");

    let mut settings = if let Some(settings_value) = store.get("settings") {
        // Parse the entire settings object
        match serde_json::from_value::<AppSettings>(settings_value) {
            Ok(settings) => {
                println!("Found existing settings: {:?}", settings);

                settings
            }
            Err(e) => {
                println!("Failed to parse settings: {}", e);
                // Fall back to default settings if parsing fails
                let default_settings = get_default_settings();

                // Store the default settings
                store.set("settings", serde_json::to_value(&default_settings).unwrap());

                default_settings
            }
        }
    } else {
        // Create default settings
        let default_settings = get_default_settings();

        // Store the settings
        store.set("settings", serde_json::to_value(&default_settings).unwrap());

        default_settings
    };

    let mut settings = settings;
    settings.normalize_microphone_fields();
    settings
}

pub fn get_settings(app: &AppHandle) -> AppSettings {
    let store = app
        .store(SETTINGS_STORE_PATH)
        .expect("Failed to initialize store");

    let mut settings = if let Some(settings_value) = store.get("settings") {
        serde_json::from_value::<AppSettings>(settings_value)
            .unwrap_or_else(|_| get_default_settings())
    } else {
        get_default_settings()
    };

    settings.normalize_microphone_fields();
    settings
}

pub fn write_settings(app: &AppHandle, settings: AppSettings) {
    let store = app
        .store(SETTINGS_STORE_PATH)
        .expect("Failed to initialize store");

    let mut normalized = settings;
    normalized.normalize_microphone_fields();

    store.set("settings", serde_json::to_value(&normalized).unwrap());
}

pub fn get_bindings(app: &AppHandle) -> HashMap<String, ShortcutBinding> {
    let settings = get_settings(app);

    settings.bindings
}

pub fn get_stored_binding(app: &AppHandle, id: &str) -> ShortcutBinding {
    let bindings = get_bindings(app);

    let binding = bindings.get(id).unwrap().clone();

    binding
}

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
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

/* still handy for composing the initial JSON in the store ------------- */
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AppSettings {
    pub bindings: HashMap<String, ShortcutBinding>,
    pub push_to_talk: bool,
    pub audio_feedback: bool,
    #[serde(default = "default_model")]
    pub selected_model: String,
    #[serde(default = "default_always_on_microphone")]
    pub always_on_microphone: bool,
    #[serde(default = "default_translate_to_english")]
    pub translate_to_english: bool,
}

fn default_model() -> String {
    // Default to empty string if no models are available yet
    // The UI will handle prompting for model download
    "".to_string()
}

fn default_always_on_microphone() -> bool {
    // Default to false for better user experience
    // True would be the old behavior (always-on for low latency)
    false
}

fn default_translate_to_english() -> bool {
    // Default to false - users need to opt-in to translation
    false
}

pub const SETTINGS_STORE_PATH: &str = "settings_store.json";

pub fn get_default_settings() -> AppSettings {
    // Set platform-specific default keyboard shortcuts
    #[cfg(target_os = "windows")]
    let default_shortcut = "ctrl+space";
    #[cfg(target_os = "macos")]
    let default_shortcut = "alt+space";  // Alt key on macOS (Option key)
    #[cfg(target_os = "linux")]
    let default_shortcut = "ctrl+space";
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    let default_shortcut = "alt+space";  // Fallback for other platforms
    
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
    // bindings.insert(
    //     "test".to_string(),
    //     ShortcutBinding {
    //         id: "test".to_string(),
    //         name: "Test".to_string(),
    //         description: "This is a test binding.".to_string(),
    //         default_binding: "ctrl+d".to_string(),
    //         current_binding: "ctrl+d".to_string(),
    //     },
    // );

    AppSettings {
        bindings,
        push_to_talk: true,
        audio_feedback: false,
        selected_model: "".to_string(),
        always_on_microphone: false,
        translate_to_english: false,
    }
}

pub fn load_or_create_app_settings(app: &App) -> AppSettings {
    // Initialize store
    let store = app
        .store(SETTINGS_STORE_PATH)
        .expect("Failed to initialize store");

    let settings = if let Some(settings_value) = store.get("settings") {
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

    settings
}

pub fn get_settings(app: &AppHandle) -> AppSettings {
    let store = app
        .store(SETTINGS_STORE_PATH)
        .expect("Failed to initialize store");

    if let Some(settings_value) = store.get("settings") {
        serde_json::from_value::<AppSettings>(settings_value).unwrap_or_else(|_| get_default_settings())
    } else {
        get_default_settings()
    }
}

pub fn write_settings(app: &AppHandle, settings: AppSettings) {
    let store = app
        .store(SETTINGS_STORE_PATH)
        .expect("Failed to initialize store");

    store.set("settings", serde_json::to_value(&settings).unwrap());
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

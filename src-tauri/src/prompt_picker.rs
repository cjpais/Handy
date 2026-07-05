use crate::overlay::get_monitor_with_cursor;
use crate::settings::LLMPrompt;
use log::debug;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::sync::Mutex;
use tauri::{AppHandle, Manager};
use tauri_specta::Event;
use tokio::sync::oneshot;

#[cfg(not(target_os = "macos"))]
use tauri::WebviewWindowBuilder;

#[cfg(target_os = "macos")]
use tauri::WebviewUrl;

#[cfg(target_os = "macos")]
use tauri_nspanel::{
    tauri_panel, CollectionBehavior, ManagerExt, PanelBuilder, PanelLevel, StyleMask,
};

const PROMPT_PICKER_WIDTH: f64 = 560.0;
const PROMPT_PICKER_HEIGHT: f64 = 320.0;

// Unlike the recording overlay's panel (which never wants keyboard focus), this
// one must — arrow keys / digits / Enter drive the list. A plain focusable
// NSWindow would work for that, but making a regular window key also activates
// the whole app, which (once the picker hides again and Handy has no other
// visible window) makes macOS deliver `applicationShouldHandleReopen:`, and
// Handy's Reopen handler unconditionally re-shows the main window — stealing
// the paste target out from under the just-finished transcription. A
// nonactivating NSPanel with `can_become_key_window: true` gets keyboard focus
// without that side effect (the same trick Spotlight-style panels use).
#[cfg(target_os = "macos")]
tauri_panel! {
    panel!(PromptPickerPanel {
        config: {
            can_become_key_window: true,
            is_floating_panel: true
        }
    })
}

pub enum PromptChoiceResult {
    Chosen(String),
    Cancelled,
}

#[derive(Default)]
pub struct PendingPromptChoice(Mutex<Option<oneshot::Sender<PromptChoiceResult>>>);

#[derive(Clone, Debug, Serialize, Deserialize, Type, tauri_specta::Event)]
pub struct PromptPickerShowEvent {
    pub prompts: Vec<LLMPrompt>,
    pub last_used_prompt_id: Option<String>,
}

/// Creates the prompt picker window and keeps it hidden by default.
#[cfg(not(target_os = "macos"))]
pub fn create_prompt_picker_window(app_handle: &AppHandle) {
    let mut builder = WebviewWindowBuilder::new(
        app_handle,
        "prompt_picker",
        tauri::WebviewUrl::App("src/prompt-picker/index.html".into()),
    )
    .title("Select Prompt")
    .resizable(false)
    .inner_size(PROMPT_PICKER_WIDTH, PROMPT_PICKER_HEIGHT)
    .shadow(false)
    .maximizable(false)
    .minimizable(false)
    .closable(false)
    .decorations(false)
    .always_on_top(true)
    .skip_taskbar(true)
    .transparent(true)
    .visible(false);

    if let Some(data_dir) = crate::portable::data_dir() {
        builder = builder.data_directory(data_dir.join("webview"));
    }

    if let Err(e) = builder.build() {
        debug!("Failed to create prompt picker window: {}", e);
    }
}

/// Creates the prompt picker panel and keeps it hidden by default (macOS).
/// A nonactivating panel that can still become key window — see the comment
/// on `PromptPickerPanel` above for why this shape is required here.
#[cfg(target_os = "macos")]
pub fn create_prompt_picker_window(app_handle: &AppHandle) {
    match PanelBuilder::<_, PromptPickerPanel>::new(app_handle, "prompt_picker")
        .url(WebviewUrl::App("src/prompt-picker/index.html".into()))
        .title("Select Prompt")
        .size(tauri::Size::Logical(tauri::LogicalSize {
            width: PROMPT_PICKER_WIDTH,
            height: PROMPT_PICKER_HEIGHT,
        }))
        .level(PanelLevel::Status)
        .has_shadow(false)
        .transparent(true)
        .no_activate(true)
        .corner_radius(0.0)
        .style_mask(StyleMask::empty().borderless().nonactivating_panel())
        .with_window(|w| w.decorations(false).transparent(true))
        .collection_behavior(
            CollectionBehavior::new()
                .can_join_all_spaces()
                .full_screen_auxiliary(),
        )
        .build()
    {
        Ok(panel) => {
            panel.hide();
        }
        Err(e) => {
            log::error!("Failed to create prompt picker panel: {}", e);
        }
    }
}

fn calculate_centered_position(
    app_handle: &AppHandle,
    width: f64,
    height: f64,
) -> Option<(f64, f64)> {
    let monitor = get_monitor_with_cursor(app_handle)?;
    let scale = monitor.scale_factor();
    let monitor_x = monitor.position().x as f64 / scale;
    let monitor_y = monitor.position().y as f64 / scale;
    let monitor_width = monitor.size().width as f64 / scale;
    let monitor_height = monitor.size().height as f64 / scale;

    let x = monitor_x + (monitor_width - width) / 2.0;
    let y = monitor_y + (monitor_height - height) / 2.0;
    Some((x, y))
}

fn show_prompt_picker(
    app_handle: &AppHandle,
    prompts: Vec<LLMPrompt>,
    last_used_prompt_id: Option<String>,
) {
    if let Some((x, y)) =
        calculate_centered_position(app_handle, PROMPT_PICKER_WIDTH, PROMPT_PICKER_HEIGHT)
    {
        // Repositioning goes through the generic window handle regardless of
        // platform — `PanelBuilder` registers a real Tauri window under the
        // hood (see `create_prompt_picker_window`), and `Panel` has no
        // position setter of its own.
        if let Some(window) = app_handle.get_webview_window("prompt_picker") {
            let _ = window.set_position(tauri::Position::Logical(tauri::LogicalPosition { x, y }));
        }
    }

    #[cfg(target_os = "macos")]
    {
        // Tauri/tao's generic `WebviewWindow::set_focus()` unconditionally
        // calls `NSApp.activateIgnoringOtherApps(true)` on macOS, which would
        // activate the whole app regardless of the panel's nonactivating
        // style mask — exactly what this window must avoid (see the comment
        // on `PromptPickerPanel`). The panel's own `show_and_make_key()`
        // gives it keyboard focus without ever touching `NSApplication`.
        //
        // Unlike the generic Tauri window API (which dispatches safely across
        // threads), `Panel` trait methods are raw AppKit calls and must run on
        // the main thread — this fires from inside the transcription
        // pipeline's async task, on a tokio worker thread, so it needs
        // `run_on_main_thread` or it crashes AppKit with "Must only be used
        // from the main thread".
        let app_handle_clone = app_handle.clone();
        let _ = app_handle.run_on_main_thread(move || {
            match app_handle_clone.get_webview_panel("prompt_picker") {
                Ok(panel) => {
                    panel.show_and_make_key();
                    let _ = PromptPickerShowEvent {
                        prompts,
                        last_used_prompt_id,
                    }
                    .emit(&app_handle_clone);
                }
                Err(e) => {
                    debug!("Prompt picker panel not found, skipping picker: {:?}", e);
                }
            }
        });
    }
    #[cfg(not(target_os = "macos"))]
    {
        let Some(window) = app_handle.get_webview_window("prompt_picker") else {
            debug!("Prompt picker window not found, skipping picker");
            return;
        };
        let _ = window.show();
        let _ = window.set_focus();
        let _ = PromptPickerShowEvent {
            prompts,
            last_used_prompt_id,
        }
        .emit(app_handle);
    }
}

fn hide_prompt_picker(app_handle: &AppHandle) {
    if let Some(window) = app_handle.get_webview_window("prompt_picker") {
        let _ = window.hide();
    }
}

/// Shows the picker and waits for the user's choice. Resolves to `Cancelled`
/// if the window is closed/hidden without an explicit choice (e.g. the sender
/// is dropped without being used, which can't happen in practice since
/// `cancel_prompt_choice` always sends before hiding, but is the correct
/// fallback if it ever did).
pub async fn await_prompt_choice(
    app_handle: &AppHandle,
    prompts: Vec<LLMPrompt>,
    last_used_prompt_id: Option<String>,
) -> PromptChoiceResult {
    let (tx, rx) = oneshot::channel();
    {
        let state = app_handle.state::<PendingPromptChoice>();
        *state.0.lock().unwrap() = Some(tx);
    }
    show_prompt_picker(app_handle, prompts, last_used_prompt_id);
    rx.await.unwrap_or(PromptChoiceResult::Cancelled)
}

#[tauri::command]
#[specta::specta]
pub fn submit_prompt_choice(app: AppHandle, prompt_id: String) -> Result<(), String> {
    let sender = app.state::<PendingPromptChoice>().0.lock().unwrap().take();
    hide_prompt_picker(&app);
    match sender {
        Some(tx) => {
            let _ = tx.send(PromptChoiceResult::Chosen(prompt_id));
            Ok(())
        }
        None => Err("No pending prompt choice".to_string()),
    }
}

#[tauri::command]
#[specta::specta]
pub fn cancel_prompt_choice(app: AppHandle) -> Result<(), String> {
    let sender = app.state::<PendingPromptChoice>().0.lock().unwrap().take();
    hide_prompt_picker(&app);
    if let Some(tx) = sender {
        let _ = tx.send(PromptChoiceResult::Cancelled);
    }
    Ok(())
}

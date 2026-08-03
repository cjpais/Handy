use crate::cli::{RecordingCommand, RemoteCommand, TranscriptCommand};
use crate::managers::history::HistoryManager;
use crate::transcription_coordinator::RecordingControl;
use crate::{clipboard, utils, TranscriptionCoordinator};
use log::{error, warn};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_clipboard_manager::ClipboardExt;

const SOURCE: &str = "CLI";
const STANDARD_BINDING: &str = "transcribe";
const POST_PROCESS_BINDING: &str = "transcribe_with_post_process";

pub(crate) fn dispatch(app: &AppHandle, command: RemoteCommand) {
    if let Err(err) = execute(app, command) {
        error!("Failed to execute remote command: {err}");
    }
}

fn execute(app: &AppHandle, command: RemoteCommand) -> Result<(), String> {
    match command {
        RemoteCommand::Recording { action } => execute_recording(app, action),
        RemoteCommand::Transcript { action } => execute_transcript(app, action),
    }
}

fn execute_recording(app: &AppHandle, action: RecordingCommand) -> Result<(), String> {
    let (control, post_process) = match action {
        RecordingCommand::Start { post_process } => (RecordingControl::Start, post_process),
        RecordingCommand::Stop => (RecordingControl::Stop, false),
        RecordingCommand::Toggle { post_process } => (RecordingControl::Toggle, post_process),
        RecordingCommand::Cancel => {
            utils::cancel_current_operation(app);
            return Ok(());
        }
    };
    let binding_id = if post_process {
        POST_PROCESS_BINDING
    } else {
        STANDARD_BINDING
    };

    let coordinator = app
        .try_state::<TranscriptionCoordinator>()
        .ok_or("TranscriptionCoordinator not initialized")?;
    coordinator.send_recording_control(control, binding_id, SOURCE);
    Ok(())
}

fn execute_transcript(app: &AppHandle, action: TranscriptCommand) -> Result<(), String> {
    let history_manager = app.state::<Arc<HistoryManager>>();
    let Some(text) = history_manager
        .get_latest_completed_transcript()
        .map_err(|err| format!("Failed to fetch last completed transcription entry: {err}"))?
    else {
        warn!("No completed transcription history entry is available");
        return Ok(());
    };

    match action {
        TranscriptCommand::Clipboard => app
            .clipboard()
            .write_text(text)
            .map_err(|err| format!("Failed to copy transcript to clipboard: {err}")),
        TranscriptCommand::Paste { method } => {
            let method_override = method.map(Into::into);
            let app_handle = app.clone();
            app.run_on_main_thread(move || {
                if let Err(err) =
                    clipboard::paste_with_method_override(text, app_handle.clone(), method_override)
                {
                    error!("Failed to paste transcript: {err}");
                    let _ = app_handle.emit("paste-error", ());
                }
            })
            .map_err(|err| format!("Failed to schedule transcript paste: {err}"))
        }
    }
}

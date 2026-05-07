use crate::actions::process_transcription_output;
use crate::audio_toolkit::{apply_custom_words, filter_transcription_output};
use crate::managers::history::{HistoryEntryMetadata, HistoryManager, HistoryProvider};
use crate::settings::{get_settings, AppSettings};
use crate::tray::{change_tray_icon, TrayIconState};
use crate::utils::{self, show_processing_overlay};
use log::{debug, error, warn};
use std::sync::Arc;
use std::time::Instant;
use tauri::{AppHandle, Emitter};

pub struct TranscriptionOutcome {
    pub raw_text: String,
    pub samples: Option<Vec<f32>>,
    pub metadata: HistoryEntryMetadata,
}

pub struct TranscriptionFinalizeOptions {
    pub post_process: bool,
    pub provider_label_for_logs: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TranscriptionFinalizeStatus {
    Completed,
    SkippedEmpty,
    PasteFailed,
}

/// Strip invisible Unicode characters that some LLMs or streaming providers may insert.
pub(crate) fn strip_invisible_chars(s: &str) -> String {
    s.replace(['\u{200B}', '\u{200C}', '\u{200D}', '\u{FEFF}'], "")
}

pub fn clean_transcription_text(settings: &AppSettings, raw_text: &str) -> String {
    let stripped = strip_invisible_chars(raw_text);
    let corrected = if settings.custom_words.is_empty() {
        stripped
    } else {
        apply_custom_words(
            &stripped,
            &settings.custom_words,
            settings.word_correction_threshold,
        )
    };

    filter_transcription_output(
        &corrected,
        &settings.app_language,
        &settings.custom_filler_words,
    )
}

fn cleanup_finalization_ui(app: &AppHandle) {
    utils::hide_recording_overlay(app);
    change_tray_icon(app, TrayIconState::Idle);
}

pub(crate) async fn save_recording_wav(
    history_manager: Arc<HistoryManager>,
    samples: Vec<f32>,
    provider_label_for_logs: &'static str,
) -> Option<String> {
    if samples.is_empty() {
        debug!(
            "{} recording produced no audio samples; skipping WAV persistence",
            provider_label_for_logs
        );
        return None;
    }

    let sample_count = samples.len();
    let file_name = format!("handy-{}.wav", chrono::Utc::now().timestamp());
    let wav_path = history_manager.recordings_dir().join(&file_name);
    let wav_path_for_verify = wav_path.clone();
    let wav_handle = tauri::async_runtime::spawn_blocking(move || {
        crate::audio_toolkit::save_wav_file(&wav_path, &samples)
    });

    let wav_saved = match wav_handle.await {
        Ok(Ok(())) => {
            match crate::audio_toolkit::verify_wav_file(&wav_path_for_verify, sample_count) {
                Ok(()) => true,
                Err(e) => {
                    error!("{} WAV verification failed: {}", provider_label_for_logs, e);
                    false
                }
            }
        }
        Ok(Err(e)) => {
            error!("Failed to save {} WAV file: {}", provider_label_for_logs, e);
            false
        }
        Err(e) => {
            error!("{} WAV save task panicked: {}", provider_label_for_logs, e);
            false
        }
    };

    wav_saved.then_some(file_name)
}

async fn paste_final_text(
    app: &AppHandle,
    final_text: String,
    provider_label_for_logs: &'static str,
) -> Result<(), String> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    let app_for_paste = app.clone();
    let app_for_cleanup = app.clone();
    let paste_time = Instant::now();

    app.run_on_main_thread(move || {
        let paste_result = match utils::paste(final_text, app_for_paste.clone()) {
            Ok(()) => {
                debug!(
                    "{} text pasted successfully in {:?}",
                    provider_label_for_logs,
                    paste_time.elapsed()
                );
                Ok(())
            }
            Err(e) => {
                error!(
                    "Failed to paste {} transcription: {}",
                    provider_label_for_logs, e
                );
                let _ = app_for_paste.emit("paste-error", ());
                Err(e)
            }
        };
        cleanup_finalization_ui(&app_for_cleanup);
        let _ = tx.send(paste_result);
    })
    .map_err(|e| {
        cleanup_finalization_ui(app);
        format!("Failed to run paste on main thread: {e:?}")
    })?;

    rx.await
        .map_err(|e| format!("Paste result channel closed: {e}"))?
}

/// finalize_transcription_outcome owns overlay/tray cleanup after it is called.
pub async fn finalize_transcription_outcome(
    app: AppHandle,
    history_manager: Arc<HistoryManager>,
    outcome: TranscriptionOutcome,
    options: TranscriptionFinalizeOptions,
) -> Result<TranscriptionFinalizeStatus, String> {
    let app_settings = get_settings(&app);
    let transcription = clean_transcription_text(&app_settings, &outcome.raw_text);

    if transcription.trim().is_empty() {
        warn!(
            "{} returned empty final text; skipping paste",
            options.provider_label_for_logs
        );
        cleanup_finalization_ui(&app);
        return Ok(TranscriptionFinalizeStatus::SkippedEmpty);
    }

    let history_file_name = match outcome.samples {
        Some(samples) => {
            save_recording_wav(
                Arc::clone(&history_manager),
                samples,
                options.provider_label_for_logs,
            )
            .await
        }
        None => None,
    };

    if options.post_process {
        show_processing_overlay(&app);
    }
    let processed = process_transcription_output(&app, &transcription, options.post_process).await;

    if let Some(file_name) = history_file_name {
        let save_result = if outcome.metadata.provider == HistoryProvider::Local {
            history_manager.save_entry(
                file_name,
                transcription,
                options.post_process,
                processed.post_processed_text.clone(),
                processed.post_process_prompt.clone(),
            )
        } else {
            history_manager.save_entry_with_metadata(
                file_name,
                transcription,
                options.post_process,
                processed.post_processed_text.clone(),
                processed.post_process_prompt.clone(),
                outcome.metadata,
            )
        };

        if let Err(err) = save_result {
            error!(
                "Failed to save {} history entry: {}",
                options.provider_label_for_logs, err
            );
        }
    }

    if processed.final_text.trim().is_empty() {
        warn!(
            "{} processed final text is empty; skipping paste",
            options.provider_label_for_logs
        );
        cleanup_finalization_ui(&app);
        return Ok(TranscriptionFinalizeStatus::SkippedEmpty);
    }

    match paste_final_text(&app, processed.final_text, options.provider_label_for_logs).await {
        Ok(()) => Ok(TranscriptionFinalizeStatus::Completed),
        Err(err) => {
            error!("{}", err);
            Ok(TranscriptionFinalizeStatus::PasteFailed)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::managers::history::HistoryProvider;
    use crate::settings::get_default_settings;

    #[test]
    fn clean_transcription_text_applies_custom_words() {
        let mut settings = get_default_settings();
        settings.custom_words = vec!["ChargeBee".to_string()];
        settings.word_correction_threshold = 0.5;

        let cleaned = clean_transcription_text(&settings, "charge bee invoice");

        assert_eq!(cleaned, "ChargeBee invoice");
    }

    #[test]
    fn clean_transcription_text_filters_fillers() {
        let mut settings = get_default_settings();
        settings.app_language = "en".to_string();

        let cleaned = clean_transcription_text(&settings, "um hello world");

        assert_eq!(cleaned, "hello world");
    }

    #[test]
    fn transcription_outcome_preserves_livestt_metadata() {
        let outcome = TranscriptionOutcome {
            raw_text: "hello".to_string(),
            samples: None,
            metadata: HistoryEntryMetadata::livestt(Some(123), Some(456)),
        };

        assert_eq!(outcome.metadata.provider, HistoryProvider::Livestt);
        assert_eq!(outcome.metadata.livestt_session_id, Some(123));
        assert_eq!(outcome.metadata.livestt_consultation_id, Some(456));
    }

    #[test]
    fn transcription_outcome_preserves_local_metadata() {
        let outcome = TranscriptionOutcome {
            raw_text: "hello".to_string(),
            samples: None,
            metadata: HistoryEntryMetadata::local(),
        };

        assert_eq!(outcome.metadata.provider, HistoryProvider::Local);
        assert_eq!(outcome.metadata.livestt_session_id, None);
        assert_eq!(outcome.metadata.livestt_consultation_id, None);
    }
}

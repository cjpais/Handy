use std::sync::{mpsc, Arc};
use std::thread;

use log::{debug, error};
use tauri::AppHandle;
use transcribe_rs::transcriber::{
    EnergyAdaptiveChunked, EnergyAdaptiveConfig, Transcriber, VadChunked, VadChunkedConfig,
};
use transcribe_rs::vad::SmoothedVad;
use transcribe_rs::TranscribeOptions;

use crate::managers::transcription::TranscriptionManager;
use crate::settings::TranscriptionMode;
use crate::utils;

pub struct StreamingFinishResult {
    pub audio: Vec<f32>,
    pub combined_text: String,
}

pub struct StreamingSession {
    #[allow(dead_code)]
    mode: TranscriptionMode,
    tm: Arc<TranscriptionManager>,
    #[allow(dead_code)]
    app: AppHandle,
    audio_buf: Arc<std::sync::Mutex<Vec<f32>>>,
    text_buf: Arc<std::sync::Mutex<Vec<String>>>,
    worker_handle: Option<thread::JoinHandle<()>>,
    active: bool,
}

impl StreamingSession {
    pub fn start(
        mode: TranscriptionMode,
        tm: Arc<TranscriptionManager>,
        app: AppHandle,
        chunk_rx: mpsc::Receiver<Vec<f32>>,
        vad_model_path: String,
        realtime_chunk_duration_secs: f32,
    ) -> Self {
        let audio_buf = Arc::new(std::sync::Mutex::new(Vec::new()));
        let text_buf = Arc::new(std::sync::Mutex::new(Vec::new()));

        let audio_buf_worker = Arc::clone(&audio_buf);
        let text_buf_worker = Arc::clone(&text_buf);
        let tm_worker = Arc::clone(&tm);
        let app_worker = app.clone();
        let pastes_live = mode == TranscriptionMode::Stream || mode == TranscriptionMode::Realtime;

        tm.begin_streaming();

        let worker_handle = thread::spawn(move || {
            let transcriber_result = create_transcriber(
                mode,
                &vad_model_path,
                realtime_chunk_duration_secs,
            );

            let mut transcriber = match transcriber_result {
                Ok(t) => t,
                Err(e) => {
                    error!("Failed to create transcriber: {}", e);
                    return;
                }
            };

            // Track text already pasted by feed() so we can extract the
            // remainder from finish() for live-paste modes.
            let mut feed_texts: Vec<String> = Vec::new();

            for chunk in chunk_rx.iter() {
                // Accumulate raw audio for history
                audio_buf_worker.lock().unwrap().extend_from_slice(&chunk);

                // Feed to the transcriber via engine access
                let feed_result = tm_worker.with_engine(|model| {
                    let results = transcriber
                        .feed(model, &chunk)
                        .map_err(|e| anyhow::anyhow!("Transcriber feed error: {}", e))?;
                    Ok(results)
                });

                match feed_result {
                    Ok(results) => {
                        for result in results {
                            let text = result.text.trim().to_string();
                            if !text.is_empty() {
                                debug!("Streaming chunk transcribed: '{}'", text);
                                feed_texts.push(text.clone());

                                if pastes_live {
                                    paste_on_main_thread(&app_worker, text);
                                }
                            }
                        }
                    }
                    Err(e) => error!("Streaming transcription error: {}", e),
                }
            }

            // Channel closed — finish() transcribes the remainder and returns
            // ALL chunks merged (feed results + remainder).
            let finish_result = tm_worker.with_engine(|model| {
                let result = transcriber
                    .finish(model)
                    .map_err(|e| anyhow::anyhow!("Transcriber finish error: {}", e))?;
                Ok(result)
            });

            match finish_result {
                Ok(result) => {
                    let full_text = result.text.trim().to_string();
                    if !full_text.is_empty() {
                        debug!("Streaming session complete: '{}'", full_text);
                        // Use finish()'s merged result as the authoritative combined text
                        *text_buf_worker.lock().unwrap() = vec![full_text.clone()];

                        // For live-paste modes: extract the remainder that wasn't
                        // already pasted by feed() and paste it now
                        if pastes_live {
                            let already_pasted = feed_texts.join(" ");
                            let remainder = if full_text.len() > already_pasted.len() {
                                full_text[already_pasted.len()..].trim()
                            } else {
                                ""
                            };
                            if !remainder.is_empty() {
                                debug!("Pasting remainder: '{}'", remainder);
                                paste_on_main_thread(&app_worker, remainder.to_string());
                            }
                        }
                    }
                }
                Err(e) => error!("Streaming finish error: {}", e),
            }

            debug!("Streaming consumer thread exiting");
        });

        Self {
            mode,
            tm,
            app,
            audio_buf,
            text_buf,
            worker_handle: Some(worker_handle),
            active: true,
        }
    }

    /// Wait for the worker thread to complete and return the combined result.
    pub fn finish(mut self) -> StreamingFinishResult {
        // Wait for the worker to finish (it exits when chunk_rx is dropped)
        if let Some(handle) = self.worker_handle.take() {
            let _ = handle.join();
        }

        let audio = std::mem::take(&mut *self.audio_buf.lock().unwrap());
        let combined_text = self.text_buf.lock().unwrap().join(" ");

        self.active = false;
        self.tm.end_streaming();

        StreamingFinishResult {
            audio,
            combined_text,
        }
    }
}

impl Drop for StreamingSession {
    fn drop(&mut self) {
        if self.active {
            self.tm.end_streaming();
        }
    }
}

fn create_transcriber(
    mode: TranscriptionMode,
    vad_model_path: &str,
    realtime_chunk_duration_secs: f32,
) -> Result<Box<dyn Transcriber>, anyhow::Error> {
    let options = TranscribeOptions::default();

    match mode {
        TranscriptionMode::Realtime => {
            let config = EnergyAdaptiveConfig {
                target_chunk_secs: realtime_chunk_duration_secs,
                search_window_secs: 1.0,
                padding_secs: 0.0,
                min_chunk_secs: 1.0,
                frame_size: 480,
                merge_separator: " ".into(),
            };
            Ok(Box::new(EnergyAdaptiveChunked::new(config, options)))
        }
        TranscriptionMode::Stream | TranscriptionMode::BatchStream => {
            let silero = transcribe_rs::vad::SileroVad::new(vad_model_path, 0.3)
                .map_err(|e| anyhow::anyhow!("Failed to create SileroVad: {}", e))?;
            let vad = SmoothedVad::new(Box::new(silero), 15, 15, 2);
            let config = VadChunkedConfig {
                min_chunk_secs: 1.0,
                max_chunk_secs: 30.0,
                padding_secs: 0.0,
                smart_split_search_secs: Some(3.0),
                merge_separator: " ".into(),
            };
            Ok(Box::new(VadChunked::new(Box::new(vad), config, options)))
        }
        TranscriptionMode::Standard => {
            unreachable!("Standard mode should not create a streaming session")
        }
    }
}

fn paste_on_main_thread(app: &AppHandle, text: String) {
    let app_clone = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Err(e) = utils::paste(text, app_clone.clone()) {
            error!("Failed to paste streamed chunk: {}", e);
        }
    });
}

use std::sync::{mpsc, Arc, Mutex};
use std::thread;

use log::{debug, error};
use tauri::AppHandle;

use crate::managers::transcription::TranscriptionManager;
use crate::settings::VadMode;
use crate::utils;

const WHISPER_SAMPLE_RATE: usize = 16000;

pub struct StreamingFinishResult {
    pub audio: Vec<f32>,
    pub combined_text: String,
}

pub struct StreamingSession {
    mode: VadMode,
    tm: Arc<TranscriptionManager>,
    app: AppHandle,
    audio_buf: Arc<Mutex<Vec<f32>>>,
    text_buf: Arc<Mutex<Vec<String>>>,
    worker_handle: Option<std::thread::JoinHandle<()>>,
    active: bool,
}

impl StreamingSession {
    pub fn start(
        mode: VadMode,
        tm: Arc<TranscriptionManager>,
        app: AppHandle,
        chunk_rx: mpsc::Receiver<Vec<f32>>,
    ) -> Self {
        let audio_buf = Arc::new(Mutex::new(Vec::new()));
        let text_buf = Arc::new(Mutex::new(Vec::new()));

        let audio_buf_worker = Arc::clone(&audio_buf);
        let text_buf_worker = Arc::clone(&text_buf);
        let tm_worker = Arc::clone(&tm);
        let app_worker = app.clone();
        let is_stream = mode == VadMode::Stream;

        tm.begin_streaming();

        let worker_handle = thread::spawn(move || {
            for chunk in chunk_rx.iter() {
                audio_buf_worker.lock().unwrap().extend_from_slice(&chunk);

                let samples = pad_short_chunk(chunk);
                match tm_worker.transcribe(samples) {
                    Ok(text) => {
                        if !text.is_empty() {
                            debug!("Streaming chunk transcribed: '{}'", text);
                            text_buf_worker.lock().unwrap().push(text.clone());

                            if is_stream {
                                paste_on_main_thread(&app_worker, text);
                            }
                        }
                    }
                    Err(e) => error!("Streaming chunk transcription error: {}", e),
                }
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

    pub fn finish(mut self, remainder: Vec<f32>) -> StreamingFinishResult {
        if let Some(handle) = self.worker_handle.take() {
            let _ = handle.join();
        }

        let remainder_text = transcribe_remainder(&self.tm, remainder.clone());
        if let Some(text) = remainder_text {
            self.text_buf.lock().unwrap().push(text.clone());
            if self.mode == VadMode::Stream {
                paste_on_main_thread(&self.app, text);
            }
        }

        let mut audio = std::mem::take(&mut *self.audio_buf.lock().unwrap());
        audio.extend_from_slice(&remainder);

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

fn pad_short_chunk(samples: Vec<f32>) -> Vec<f32> {
    if samples.len() < WHISPER_SAMPLE_RATE {
        let mut padded = samples;
        padded.resize(WHISPER_SAMPLE_RATE * 5 / 4, 0.0);
        padded
    } else {
        samples
    }
}

fn transcribe_remainder(
    tm: &Arc<TranscriptionManager>,
    samples: Vec<f32>,
) -> Option<String> {
    if samples.is_empty() {
        return None;
    }
    let samples = pad_short_chunk(samples);
    match tm.transcribe(samples) {
        Ok(text) if !text.is_empty() => {
            debug!("Final streaming remainder transcribed: '{}'", text);
            Some(text)
        }
        Ok(_) => None,
        Err(e) => {
            error!("Final streaming remainder transcription error: {}", e);
            None
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

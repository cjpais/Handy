use crate::audio_toolkit::{list_input_devices, vad::SmoothedVad, AudioRecorder, SileroVad};
use crate::settings::{get_settings, MicrophoneKeepAlive};
use crate::utils;
use log::{debug, info};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use tauri::{App, Manager};

const WHISPER_SAMPLE_RATE: usize = 16000;

/* ──────────────────────────────────────────────────────────────── */

#[derive(Clone, Debug)]
pub enum RecordingState {
    Idle,
    Recording { binding_id: String },
}

fn create_audio_recorder(
    vad_path: &str,
    app_handle: &tauri::AppHandle,
) -> Result<AudioRecorder, anyhow::Error> {
    let silero = SileroVad::new(vad_path, 0.3)
        .map_err(|e| anyhow::anyhow!("Failed to create SileroVad: {}", e))?;
    let smoothed_vad = SmoothedVad::new(Box::new(silero), 15, 15, 2);

    // Recorder with VAD plus a spectrum-level callback that forwards updates to
    // the frontend.
    let recorder = AudioRecorder::new()
        .map_err(|e| anyhow::anyhow!("Failed to create AudioRecorder: {}", e))?
        .with_vad(Box::new(smoothed_vad))
        .with_level_callback({
            let app_handle = app_handle.clone();
            move |levels| {
                utils::emit_levels(&app_handle, &levels);
            }
        });

    Ok(recorder)
}

/* ──────────────────────────────────────────────────────────────── */

#[derive(Clone)]
pub struct AudioRecordingManager {
    state: Arc<Mutex<RecordingState>>,
    keep_alive: Arc<Mutex<MicrophoneKeepAlive>>,
    close_timer_generation: Arc<AtomicU64>,
    app_handle: tauri::AppHandle,

    recorder: Arc<Mutex<Option<AudioRecorder>>>,
    is_open: Arc<Mutex<bool>>,
    is_recording: Arc<Mutex<bool>>,
}

impl AudioRecordingManager {
    /* ---------- construction ------------------------------------------------ */

    pub fn new(app: &App) -> Result<Self, anyhow::Error> {
        let settings = get_settings(&app.handle());
        let keep_alive_setting = settings.microphone_keep_alive;

        let manager = Self {
            state: Arc::new(Mutex::new(RecordingState::Idle)),
            keep_alive: Arc::new(Mutex::new(keep_alive_setting)),
            close_timer_generation: Arc::new(AtomicU64::new(0)),
            app_handle: app.handle().clone(),

            recorder: Arc::new(Mutex::new(None)),
            is_open: Arc::new(Mutex::new(false)),
            is_recording: Arc::new(Mutex::new(false)),
        };

        if matches!(keep_alive_setting, MicrophoneKeepAlive::Forever) {
            manager.start_microphone_stream()?;
        }

        Ok(manager)
    }

    /* ---------- microphone life-cycle -------------------------------------- */

    pub fn start_microphone_stream(&self) -> Result<(), anyhow::Error> {
        self.cancel_close_timer();
        let mut open_flag = self.is_open.lock().unwrap();
        if *open_flag {
            debug!("Microphone stream already active");
            return Ok(());
        }

        let start_time = Instant::now();

        let vad_path = self
            .app_handle
            .path()
            .resolve(
                "resources/models/silero_vad_v4.onnx",
                tauri::path::BaseDirectory::Resource,
            )
            .map_err(|e| anyhow::anyhow!("Failed to resolve VAD path: {}", e))?;
        let mut recorder_opt = self.recorder.lock().unwrap();

        if recorder_opt.is_none() {
            *recorder_opt = Some(create_audio_recorder(
                vad_path.to_str().unwrap(),
                &self.app_handle,
            )?);
        }

        // Get the selected device from settings
        let settings = get_settings(&self.app_handle);
        let selected_device = if let Some(device_name) = settings.selected_microphone {
            // Find the device by name
            match list_input_devices() {
                Ok(devices) => devices
                    .into_iter()
                    .find(|d| d.name == device_name)
                    .map(|d| d.device),
                Err(e) => {
                    debug!("Failed to list devices, using default: {}", e);
                    None
                }
            }
        } else {
            None
        };

        if let Some(rec) = recorder_opt.as_mut() {
            rec.open(selected_device)
                .map_err(|e| anyhow::anyhow!("Failed to open recorder: {}", e))?;
        }

        *open_flag = true;
        info!(
            "Microphone stream initialized in {:?}",
            start_time.elapsed()
        );
        Ok(())
    }

    pub fn stop_microphone_stream(&self) {
        self.cancel_close_timer();
        let mut open_flag = self.is_open.lock().unwrap();
        if !*open_flag {
            return;
        }

        if let Some(rec) = self.recorder.lock().unwrap().as_mut() {
            // If still recording, stop first.
            if *self.is_recording.lock().unwrap() {
                let _ = rec.stop();
                *self.is_recording.lock().unwrap() = false;
            }
            let _ = rec.close();
        }

        *open_flag = false;
        debug!("Microphone stream stopped");
    }

    /* ---------- mode switching --------------------------------------------- */

    pub fn update_keep_alive(
        &self,
        new_keep_alive: MicrophoneKeepAlive,
    ) -> Result<(), anyhow::Error> {
        *self.keep_alive.lock().unwrap() = new_keep_alive;
        self.cancel_close_timer();

        let is_recording = *self.is_recording.lock().unwrap();
        if is_recording {
            return Ok(());
        }

        match new_keep_alive {
            MicrophoneKeepAlive::Forever => {
                self.start_microphone_stream()?;
            }
            MicrophoneKeepAlive::Off => {
                self.stop_microphone_stream();
            }
            other => {
                if let Some(duration) = other.duration() {
                    if duration.is_zero() {
                        self.stop_microphone_stream();
                    } else if *self.is_open.lock().unwrap() {
                        self.schedule_close_timer(duration);
                    }
                }
            }
        }

        Ok(())
    }

    /* ---------- recording --------------------------------------------------- */

    pub fn try_start_recording(&self, binding_id: &str) -> bool {
        let mut state = self.state.lock().unwrap();

        if let RecordingState::Idle = *state {
            self.cancel_close_timer();
            if let Err(e) = self.start_microphone_stream() {
                eprintln!("Failed to open microphone stream: {e}");
                return false;
            }

            if let Some(rec) = self.recorder.lock().unwrap().as_ref() {
                if rec.start().is_ok() {
                    *self.is_recording.lock().unwrap() = true;
                    *state = RecordingState::Recording {
                        binding_id: binding_id.to_string(),
                    };
                    debug!("Recording started for binding {binding_id}");
                    return true;
                }
            }
            eprintln!("Recorder not available");
            false
        } else {
            false
        }
    }

    pub fn update_selected_device(&self) -> Result<(), anyhow::Error> {
        // If currently open, restart the microphone stream to use the new device
        if *self.is_open.lock().unwrap() {
            self.stop_microphone_stream();
            self.start_microphone_stream()?;
        }
        Ok(())
    }

    pub fn stop_recording(&self, binding_id: &str) -> Option<Vec<f32>> {
        let mut state = self.state.lock().unwrap();

        match *state {
            RecordingState::Recording {
                binding_id: ref active,
            } if active == binding_id => {
                *state = RecordingState::Idle;
                drop(state);

                let samples = if let Some(rec) = self.recorder.lock().unwrap().as_ref() {
                    match rec.stop() {
                        Ok(buf) => buf,
                        Err(e) => {
                            eprintln!("stop() failed: {e}");
                            Vec::new()
                        }
                    }
                } else {
                    eprintln!("Recorder not available");
                    Vec::new()
                };

                *self.is_recording.lock().unwrap() = false;
                self.handle_idle_transition();

                // Pad if very short
                let s_len = samples.len();
                // println!("Got {} samples", { s_len });
                if s_len < WHISPER_SAMPLE_RATE && s_len > 0 {
                    let mut padded = samples;
                    padded.resize(WHISPER_SAMPLE_RATE * 5 / 4, 0.0);
                    Some(padded)
                } else {
                    Some(samples)
                }
            }
            _ => None,
        }
    }

    /// Cancel any ongoing recording without returning audio samples
    pub fn cancel_recording(&self) {
        let mut state = self.state.lock().unwrap();

        if let RecordingState::Recording { .. } = *state {
            *state = RecordingState::Idle;
            drop(state);

            if let Some(rec) = self.recorder.lock().unwrap().as_ref() {
                let _ = rec.stop(); // Discard the result
            }

            *self.is_recording.lock().unwrap() = false;
            self.handle_idle_transition();
        }
    }

    pub fn is_stream_open(&self) -> bool {
        *self.is_open.lock().unwrap()
    }

    fn current_keep_alive(&self) -> MicrophoneKeepAlive {
        *self.keep_alive.lock().unwrap()
    }

    fn cancel_close_timer(&self) {
        self.close_timer_generation.fetch_add(1, Ordering::SeqCst);
    }

    fn schedule_close_timer(&self, delay: Duration) {
        if delay.is_zero() {
            self.stop_microphone_stream();
            return;
        }

        let generation = self.close_timer_generation.fetch_add(1, Ordering::SeqCst) + 1;
        let manager = self.clone();
        thread::spawn(move || {
            thread::sleep(delay);
            if manager
                .close_timer_generation
                .load(Ordering::SeqCst)
                == generation
            {
                if !*manager.is_recording.lock().unwrap() {
                    manager.stop_microphone_stream();
                }
            }
        });
    }

    fn handle_idle_transition(&self) {
        if *self.is_recording.lock().unwrap() {
            return;
        }

        self.cancel_close_timer();

        match self.current_keep_alive() {
            MicrophoneKeepAlive::Forever => {
                // Keep the stream open indefinitely.
            }
            MicrophoneKeepAlive::Off => {
                self.stop_microphone_stream();
            }
            keep_alive => {
                if let Some(duration) = keep_alive.duration() {
                    if duration.is_zero() {
                        self.stop_microphone_stream();
                    } else {
                        let is_open = *self.is_open.lock().unwrap();
                        if is_open {
                            self.schedule_close_timer(duration);
                        }
                    }
                }
            }
        }
    }
}

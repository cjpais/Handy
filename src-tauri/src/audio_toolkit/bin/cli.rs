use hound::WavWriter;
use std::io::{self, Write};

use parler_app_lib::audio_toolkit::{
    audio::{list_input_devices, CpalDeviceInfo},
    vad::SmoothedVad,
    AudioRecorder, SileroVad,
};

#[derive(Debug, Clone, PartialEq)]
enum RecorderMode {
    AlwaysOn,
    OnDemand,
}

impl std::fmt::Display for RecorderMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RecorderMode::AlwaysOn => write!(f, "Always-On"),
            RecorderMode::OnDemand => write!(f, "On-Demand"),
        }
    }
}

struct RecorderState {
    recorder: AudioRecorder,
    mode: RecorderMode,
    is_recording: bool,
    is_open: bool,
    current_device_index: Option<usize>,
    recording_index: u32,
}

impl RecorderState {
    fn new(recorder: AudioRecorder) -> Self {
        Self {
            recorder,
            mode: RecorderMode::AlwaysOn,
            is_recording: false,
            is_open: false,
            current_device_index: None,
            recording_index: 1,
        }
    }

    fn switch_mode(&mut self, new_mode: RecorderMode) -> Result<(), Box<dyn std::error::Error>> {
        if self.mode == new_mode {
            return Ok(());
        }

        // If we're currently recording, stop first
        if self.is_recording {
            println!("Stopping current recording to switch modes...");
            self.stop_recording()?;
        }

        // Close if open and switching to on-demand, or if switching from on-demand to always-on
        if self.is_open {
            match (&self.mode, &new_mode) {
                (RecorderMode::AlwaysOn, RecorderMode::OnDemand) => {
                    self.recorder.close()?;
                    self.is_open = false;
                    println!("Closed recorder for On-Demand mode");
                }
                (RecorderMode::OnDemand, RecorderMode::AlwaysOn) => {
                    // For switching from on-demand to always-on, we need to reopen
                    // This will be handled when the user starts recording
                }
                _ => {}
            }
        }

        self.mode = new_mode;
        println!("Switched to {} mode", self.mode);
        Ok(())
    }

    fn start_recording(
        &mut self,
        device_index: Option<usize>,
        devices: &[CpalDeviceInfo],
    ) -> Result<(), Box<dyn std::error::Error>> {
        if self.is_recording {
            return Err("Already recording! Stop the current recording first.".into());
        }

        let device = if let Some(idx) = device_index {
            if idx >= devices.len() {
                return Err(format!(
                    "Invalid device index: {}. Available devices: 0-{}",
                    idx,
                    devices.len() - 1
                )
                .into());
            }
            Some(devices[idx].device.clone())
        } else {
            None
        };

        match self.mode {
            RecorderMode::AlwaysOn => {
                // In always-on mode, open once and keep open
                if !self.is_open || self.current_device_index != device_index {
                    if self.is_open {
                        self.recorder.close()?;
                    }
                    self.recorder.open(device)?;
                    self.is_open = true;
                    self.current_device_index = device_index;
                    println!("Opened recorder in Always-On mode");
                }
                self.recorder.start()?;
            }
            RecorderMode::OnDemand => {
                // In on-demand mode, open for each recording
                if self.is_open {
                    self.recorder.close()?;
                }
                self.recorder.open(device)?;
                self.is_open = true;
                self.current_device_index = device_index;
                self.recorder.start()?;
                println!("Opened and started recorder in On-Demand mode");
            }
        }

        self.is_recording = true;
        println!(
            "Recording started with device: {}",
            device_index.map_or("default".to_string(), |i| i.to_string())
        );
        Ok(())
    }

    fn stop_recording(&mut self) -> Result<Vec<f32>, Box<dyn std::error::Error>> {
        if !self.is_recording {
            return Err("No recording in progress.".into());
        }

        let samples = self.recorder.stop()?;
        self.is_recording = false;

        match self.mode {
            RecorderMode::AlwaysOn => {
                // Keep the recorder open for next recording
                println!("Recording stopped. Recorder remains open for next recording.");
            }
            RecorderMode::OnDemand => {
                // Close the recorder after each recording
                self.recorder.close()?;
                self.is_open = false;
                self.current_device_index = None;
                println!("Recording stopped and recorder closed.");
            }
        }

        Ok(samples)
    }

    fn close(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if self.is_recording {
            self.stop_recording()?;
        }
        if self.is_open {
            self.recorder.close()?;
            self.is_open = false;
        }
        Ok(())
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Advanced Audio Recorder CLI");
    println!("=========================");
    print_help();

    let silero = SileroVad::new("./resources/models/silero_vad_v4.onnx", 0.5)?;
    let smoothed_vad = SmoothedVad::new(Box::new(silero), 15, 15);
    let recorder = AudioRecorder::new()?.with_vad(Box::new(smoothed_vad));
    let mut state = RecorderState::new(recorder);

    let mut devices = list_input_devices()?;
    print_devices(&devices);

    loop {
        print!("[{}] > ", state.mode);
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let parts: Vec<&str> = input.trim().split_whitespace().collect();

        if parts.is_empty() {
            continue;
        }

        let command = parts[0].to_lowercase();

        match command.as_str() {
            "start" | "s" => {
                let device_index = if parts.len() > 1 {
                    match parts[1].parse::<usize>() {
                        Ok(idx) => Some(idx),
                        Err(_) => {
                            println!("Invalid device index format. Usage: start [device_index]");
                            continue;
                        }
                    }
                } else {
                    None
                };

                match state.start_recording(device_index, &devices) {
                    Ok(_) => println!("Recording started successfully!"),
                    Err(e) => println!("Error starting recording: {}", e),
                }
            }
            "stop" => match state.stop_recording() {
                Ok(samples) => {
                    if !samples.is_empty() {
                        let filename = format!("recording_{}.wav", state.recording_index);
                        match save_audio(&samples, &filename) {
                            Ok(_) => {
                                println!("Recording saved as: {}", filename);
                                state.recording_index += 1;
                            }
                            Err(e) => println!("Error saving recording: {}", e),
                        }
                    } else {
                        println!("No audio data captured.");
                    }
                }
                Err(e) => println!("Error stopping recording: {}", e),
            },
            "mode" => {
                if parts.len() > 1 {
                    let new_mode = match parts[1].to_lowercase().as_str() {
                        "always" | "alwayson" | "always-on" | "a" => RecorderMode::AlwaysOn,
                        "demand" | "ondemand" | "on-demand" | "d" => RecorderMode::OnDemand,
                        _ => {
                            println!("Invalid mode. Use 'always' or 'demand'");
                            continue;
                        }
                    };
                    match state.switch_mode(new_mode) {
                        Ok(_) => {}
                        Err(e) => println!("Error switching modes: {}", e),
                    }
                } else {
                    println!("Current mode: {}", state.mode);
                    println!("Usage: mode [always|demand]");
                }
            }
            "devices" | "dev" => {
                devices = list_input_devices()?;
                print_devices(&devices);
            }
            "status" => {
                println!("Status:");
                println!("  Mode: {}", state.mode);
                println!(
                    "  Recording: {}",
                    if state.is_recording { "Yes" } else { "No" }
                );
                println!(
                    "  Recorder Open: {}",
                    if state.is_open { "Yes" } else { "No" }
                );
                println!(
                    "  Current Device: {}",
                    state
                        .current_device_index
                        .map_or("None".to_string(), |i| i.to_string())
                );
                println!("  Next Recording: recording_{}.wav", state.recording_index);
            }
            "help" | "h" => {
                print_help();
            }
            "quit" | "exit" | "q" => {
                println!("Shutting down...");
                match state.close() {
                    Ok(_) => {
                        if state.is_recording {
                            println!(
                                "Final recording saved as: recording_{}.wav",
                                state.recording_index
                            );
                        }
                    }
                    Err(e) => println!("Error during shutdown: {}", e),
                }
                println!("Goodbye!");
                break;
            }
            "" => {
                // Empty input, continue
            }
            _ => {
                println!(
                    "Unknown command: '{}'. Type 'help' for available commands.",
                    command
                );
            }
        }
    }

    Ok(())
}

fn print_help() {
    println!("Commands:");
    println!(
        "  start [device_index] | s [device_index]  - Start recording (optionally with device)"
    );
    println!("  stop                                      - Stop recording and save");
    println!(
        "  mode [always|demand]                      - Switch recording mode or show current mode"
    );
    println!("  devices | dev                             - List available audio devices");
    println!("  status                                    - Show current recorder status");
    println!("  help | h                                  - Show this help message");
    println!("  quit | exit | q                           - Exit the program");
    println!();
    println!("Modes:");
    println!("  Always-On: Keeps recorder open for quick start/stop cycles");
    println!("  On-Demand: Opens/closes recorder for each recording session");
    println!();
}

fn print_devices(devices: &[CpalDeviceInfo]) {
    println!("Available audio devices:");
    for (index, device) in devices.iter().enumerate() {
        println!("  {}: {}", index, device.name);
    }
    println!();
}

fn save_audio(samples: &[f32], filename: &str) -> Result<(), Box<dyn std::error::Error>> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 16000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };

    let mut writer = WavWriter::create(filename, spec)?;

    for &sample in samples {
        let sample_i16 = (sample * i16::MAX as f32) as i16;
        writer.write_sample(sample_i16)?;
    }

    writer.finalize()?;
    Ok(())
}

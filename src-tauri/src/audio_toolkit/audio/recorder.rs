use std::{
    io::Error,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    time::Duration,
};

use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    Device, Sample, SizedSample,
};

use crate::audio_toolkit::{
    audio::{AudioVisualiser, FrameResampler},
    constants,
    vad::{self, VadFrame},
    VoiceActivityDetector,
};

enum Cmd {
    Start,
    Stop(mpsc::Sender<Vec<f32>>),
    Shutdown,
}

enum AudioChunk {
    Samples(Vec<f32>),
    EndOfStream,
}

pub struct AudioRecorder {
    device: Option<Device>,
    cmd_tx: Option<mpsc::Sender<Cmd>>,
    worker_handle: Option<std::thread::JoinHandle<()>>,
    vad: Option<Arc<Mutex<Box<dyn vad::VoiceActivityDetector>>>>,
    level_cb: Option<Arc<dyn Fn(Vec<f32>) + Send + Sync + 'static>>,
}

impl AudioRecorder {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        Ok(AudioRecorder {
            device: None,
            cmd_tx: None,
            worker_handle: None,
            vad: None,
            level_cb: None,
        })
    }

    pub fn with_vad(mut self, vad: Box<dyn VoiceActivityDetector>) -> Self {
        self.vad = Some(Arc::new(Mutex::new(vad)));
        self
    }

    pub fn with_level_callback<F>(mut self, cb: F) -> Self
    where
        F: Fn(Vec<f32>) + Send + Sync + 'static,
    {
        self.level_cb = Some(Arc::new(cb));
        self
    }

    pub fn open(
        &mut self,
        device: Option<Device>,
        loopback_device: Option<Device>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if self.worker_handle.is_some() {
            return Ok(()); // already open
        }

        let (sample_tx, sample_rx) = mpsc::channel::<AudioChunk>();
        let (cmd_tx, cmd_rx) = mpsc::channel::<Cmd>();
        let (init_tx, init_rx) = mpsc::sync_channel::<Result<(), String>>(1);

        let host = crate::audio_toolkit::get_cpal_host();
        let device = match device {
            Some(dev) => dev,
            None => host
                .default_input_device()
                .ok_or_else(|| Error::new(std::io::ErrorKind::NotFound, "No input device found"))?,
        };

        let loopback_channel = loopback_device.map(|dev| {
            let (tx, rx) = mpsc::channel::<AudioChunk>();
            (dev, tx, rx)
        });

        let thread_device = device.clone();
        let vad = self.vad.clone();
        let level_cb = self.level_cb.clone();

        let worker = std::thread::spawn(move || {
            let stop_flag = Arc::new(AtomicBool::new(false));
            let stop_flag_for_mic = stop_flag.clone();

            let init_result =
                (|| -> Result<(cpal::Stream, u32, Option<cpal::Stream>, Option<u32>), String> {
                    // ---- primary (microphone) stream ----
                    let config = AudioRecorder::get_preferred_config(&thread_device, false)
                        .map_err(|e| format!("Failed to fetch preferred config: {e}"))?;

                    let sample_rate = config.sample_rate().0;
                    let channels = config.channels() as usize;

                    log::info!(
                        "Using device: {:?}\nSample rate: {}\nChannels: {}\nFormat: {:?}",
                        thread_device.name(),
                        sample_rate,
                        channels,
                        config.sample_format()
                    );

                    let stream = AudioRecorder::build_stream_dynamic(
                        &thread_device,
                        &config,
                        sample_tx,
                        channels,
                        stop_flag_for_mic,
                    )
                    .map_err(|e| format!("Failed to build input stream: {e}"))?;

                    stream
                        .play()
                        .map_err(|e| format!("Failed to start microphone stream: {e}"))?;

                    // ---- optional loopback stream ----
                    let (lb_stream, lb_rate) = if let Some((lb_dev, lb_tx, _)) = &loopback_channel {
                        let lb_config = AudioRecorder::get_preferred_config(lb_dev, true)
                            .map_err(|e| format!("Failed to fetch loopback config: {e}"))?;

                        let lb_sample_rate = lb_config.sample_rate().0;
                        let lb_channels = lb_config.channels() as usize;
                        let stop_flag_for_lb = stop_flag.clone();

                        log::info!(
                            "Loopback device: {:?}\nSample rate: {}\nChannels: {}\nFormat: {:?}",
                            lb_dev.name(),
                            lb_sample_rate,
                            lb_channels,
                            lb_config.sample_format()
                        );

                        let s = AudioRecorder::build_stream_dynamic(
                            lb_dev,
                            &lb_config,
                            lb_tx.clone(),
                            lb_channels,
                            stop_flag_for_lb,
                        )
                        .map_err(|e| format!("Failed to build loopback stream: {e}"))?;

                        s.play()
                            .map_err(|e| format!("Failed to start loopback stream: {e}"))?;

                        (Some(s), Some(lb_sample_rate))
                    } else {
                        (None, None)
                    };

                    Ok((stream, sample_rate, lb_stream, lb_rate))
                })();

            match init_result {
                Ok((stream, sample_rate, lb_stream, lb_rate)) => {
                    let _ = init_tx.send(Ok(()));
                    let loopback_rx = loopback_channel.map(|(_, _, rx)| rx);
                    run_consumer(
                        sample_rate,
                        vad,
                        sample_rx,
                        cmd_rx,
                        level_cb,
                        stop_flag,
                        loopback_rx,
                        lb_rate,
                    );
                    drop(stream);
                    drop(lb_stream);
                }
                Err(error_message) => {
                    log::error!("{error_message}");
                    let _ = init_tx.send(Err(error_message));
                }
            }
        });

        match init_rx.recv() {
            Ok(Ok(())) => {
                self.device = Some(device);
                self.cmd_tx = Some(cmd_tx);
                self.worker_handle = Some(worker);
                Ok(())
            }
            Ok(Err(error_message)) => {
                let _ = worker.join();
                let kind = if is_microphone_access_denied(&error_message) {
                    std::io::ErrorKind::PermissionDenied
                } else {
                    std::io::ErrorKind::Other
                };
                Err(Box::new(Error::new(kind, error_message)))
            }
            Err(recv_error) => {
                let _ = worker.join();
                Err(Box::new(Error::new(
                    std::io::ErrorKind::Other,
                    format!("Failed to initialize microphone worker: {recv_error}"),
                )))
            }
        }
    }

    pub fn start(&self) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(tx) = &self.cmd_tx {
            tx.send(Cmd::Start)?;
        }
        Ok(())
    }

    pub fn stop(&self) -> Result<Vec<f32>, Box<dyn std::error::Error>> {
        let (resp_tx, resp_rx) = mpsc::channel();
        if let Some(tx) = &self.cmd_tx {
            tx.send(Cmd::Stop(resp_tx))?;
        }
        Ok(resp_rx.recv()?) // wait for the samples
    }

    pub fn close(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(tx) = self.cmd_tx.take() {
            let _ = tx.send(Cmd::Shutdown);
        }
        if let Some(h) = self.worker_handle.take() {
            let _ = h.join();
        }
        self.device = None;
        Ok(())
    }

    fn build_stream_dynamic(
        device: &cpal::Device,
        config: &cpal::SupportedStreamConfig,
        sample_tx: mpsc::Sender<AudioChunk>,
        channels: usize,
        stop_flag: Arc<AtomicBool>,
    ) -> Result<cpal::Stream, cpal::BuildStreamError> {
        match config.sample_format() {
            cpal::SampleFormat::U8 => {
                Self::build_stream::<u8>(device, config, sample_tx, channels, stop_flag)
            }
            cpal::SampleFormat::I8 => {
                Self::build_stream::<i8>(device, config, sample_tx, channels, stop_flag)
            }
            cpal::SampleFormat::I16 => {
                Self::build_stream::<i16>(device, config, sample_tx, channels, stop_flag)
            }
            cpal::SampleFormat::I32 => {
                Self::build_stream::<i32>(device, config, sample_tx, channels, stop_flag)
            }
            cpal::SampleFormat::F32 => {
                Self::build_stream::<f32>(device, config, sample_tx, channels, stop_flag)
            }
            _ => Err(cpal::BuildStreamError::StreamConfigNotSupported),
        }
    }

    fn build_stream<T>(
        device: &cpal::Device,
        config: &cpal::SupportedStreamConfig,
        sample_tx: mpsc::Sender<AudioChunk>,
        channels: usize,
        stop_flag: Arc<AtomicBool>,
    ) -> Result<cpal::Stream, cpal::BuildStreamError>
    where
        T: Sample + SizedSample + Send + 'static,
        f32: cpal::FromSample<T>,
    {
        let mut output_buffer = Vec::new();
        let mut eos_sent = false;

        let stream_cb = move |data: &[T], _: &cpal::InputCallbackInfo| {
            if stop_flag.load(Ordering::Relaxed) {
                if !eos_sent {
                    let _ = sample_tx.send(AudioChunk::EndOfStream);
                    eos_sent = true;
                }
                return;
            }
            eos_sent = false;

            output_buffer.clear();

            if channels == 1 {
                output_buffer.extend(data.iter().map(|&sample| sample.to_sample::<f32>()));
            } else {
                let frame_count = data.len() / channels;
                output_buffer.reserve(frame_count);

                for frame in data.chunks_exact(channels) {
                    let mono_sample = frame
                        .iter()
                        .map(|&sample| sample.to_sample::<f32>())
                        .sum::<f32>()
                        / channels as f32;
                    output_buffer.push(mono_sample);
                }
            }

            if sample_tx
                .send(AudioChunk::Samples(output_buffer.clone()))
                .is_err()
            {
                log::error!("Failed to send samples");
            }
        };

        device.build_input_stream(
            &config.clone().into(),
            stream_cb,
            |err| log::error!("Stream error: {}", err),
            None,
        )
    }

    fn get_preferred_config(
        device: &cpal::Device,
        is_loopback: bool,
    ) -> Result<cpal::SupportedStreamConfig, Box<dyn std::error::Error>> {
        // Use the device's native/default sample rate and let the FrameResampler
        // in run_consumer() downsample to 16kHz. This avoids forcing hardware into
        // a non-native rate which can cause issues on some devices (Bluetooth
        // codecs, certain ALSA drivers, etc.).
        let default_config = if is_loopback {
            device.default_output_config()?
        } else {
            device.default_input_config()?
        };
        let target_rate = default_config.sample_rate();

        // Try to find the best sample format at the device's default rate.
        // Collect into a Vec because SupportedOutputConfigs and SupportedInputConfigs
        // are distinct iterator types.
        let supported_configs: Vec<cpal::SupportedStreamConfigRange> = if is_loopback {
            match device.supported_output_configs() {
                Ok(configs) => configs.collect(),
                Err(e) => {
                    log::warn!("Could not enumerate configs ({e}), using device default");
                    return Ok(default_config);
                }
            }
        } else {
            match device.supported_input_configs() {
                Ok(configs) => configs.collect(),
                Err(e) => {
                    log::warn!("Could not enumerate configs ({e}), using device default");
                    return Ok(default_config);
                }
            }
        };
        let mut best_config: Option<cpal::SupportedStreamConfigRange> = None;

        for config_range in supported_configs {
            if config_range.min_sample_rate() <= target_rate
                && config_range.max_sample_rate() >= target_rate
            {
                match best_config {
                    None => best_config = Some(config_range),
                    Some(ref current) => {
                        // Prioritize F32 > I16 > I32 > others
                        let score = |fmt: cpal::SampleFormat| match fmt {
                            cpal::SampleFormat::F32 => 4,
                            cpal::SampleFormat::I16 => 3,
                            cpal::SampleFormat::I32 => 2,
                            _ => 1,
                        };

                        if score(config_range.sample_format()) > score(current.sample_format()) {
                            best_config = Some(config_range);
                        }
                    }
                }
            }
        }

        if let Some(config) = best_config {
            return Ok(config.with_sample_rate(target_rate));
        }

        // Fall back to device default if no config matched (exotic/virtual devices)
        log::warn!(
            "No supported config matched device default rate {:?}, using default config",
            target_rate
        );
        Ok(default_config)
    }
}

pub fn is_microphone_access_denied(error_message: &str) -> bool {
    let normalized = error_message.to_lowercase();
    normalized.contains("access is denied")
        || normalized.contains("permission denied")
        || normalized.contains("0x80070005")
}

pub fn is_no_input_device_error(error_message: &str) -> bool {
    let normalized = error_message.to_lowercase();
    normalized.contains("no input device found")
        || (normalized.contains("failed to fetch preferred config")
            && normalized.contains("coreaudio"))
}

#[cfg(test)]
mod tests {
    use super::{is_microphone_access_denied, is_no_input_device_error};

    #[test]
    fn detects_access_is_denied() {
        assert!(is_microphone_access_denied("Access is denied"));
    }

    #[test]
    fn detects_permission_denied() {
        assert!(is_microphone_access_denied("permission denied"));
    }

    #[test]
    fn detects_windows_error_code() {
        assert!(is_microphone_access_denied("WASAPI error: 0x80070005"));
    }

    #[test]
    fn does_not_match_unrelated_errors() {
        assert!(!is_microphone_access_denied("device not found"));
    }

    #[test]
    fn detects_no_input_device() {
        assert!(is_no_input_device_error("No input device found"));
    }

    #[test]
    fn detects_coreaudio_config_error() {
        assert!(is_no_input_device_error(
            "Failed to fetch preferred config: A backend-specific error has occurred: An unknown error unknown to the coreaudio-rs API occurred"
        ));
    }

    #[test]
    fn does_not_match_other_errors_for_no_device() {
        assert!(!is_no_input_device_error("permission denied"));
        assert!(!is_no_input_device_error("device not found"));
    }
}

fn run_consumer(
    in_sample_rate: u32,
    vad: Option<Arc<Mutex<Box<dyn vad::VoiceActivityDetector>>>>,
    sample_rx: mpsc::Receiver<AudioChunk>,
    cmd_rx: mpsc::Receiver<Cmd>,
    level_cb: Option<Arc<dyn Fn(Vec<f32>) + Send + Sync + 'static>>,
    stop_flag: Arc<AtomicBool>,
    loopback_rx: Option<mpsc::Receiver<AudioChunk>>,
    loopback_sample_rate: Option<u32>,
) {
    let mut frame_resampler = FrameResampler::new(
        in_sample_rate as usize,
        constants::WHISPER_SAMPLE_RATE as usize,
        Duration::from_millis(30),
    );

    let has_loopback = loopback_rx.is_some();
    let mut lb_resampler = loopback_sample_rate.map(|rate| {
        FrameResampler::new(
            rate as usize,
            constants::WHISPER_SAMPLE_RATE as usize,
            Duration::from_millis(30),
        )
    });

    let mut mic_16k_buf: Vec<f32> = Vec::new();
    let mut lb_16k_buf: Vec<f32> = Vec::new();
    const FRAME_16K: usize = (constants::WHISPER_SAMPLE_RATE as usize) * 30 / 1000; // 30ms
    let mut frame_buf: Vec<f32> = Vec::with_capacity(FRAME_16K);

    let mut processed_samples = Vec::<f32>::new();
    let mut recording = false;

    // ---------- spectrum visualisation setup ---------------------------- //
    const BUCKETS: usize = 16;
    const WINDOW_SIZE: usize = 512;
    let mut visualizer = AudioVisualiser::new(
        in_sample_rate,
        WINDOW_SIZE,
        BUCKETS,
        400.0,  // vocal_min_hz
        4000.0, // vocal_max_hz
    );

    fn handle_frame(
        samples: &[f32],
        recording: bool,
        vad: &Option<Arc<Mutex<Box<dyn vad::VoiceActivityDetector>>>>,
        out_buf: &mut Vec<f32>,
    ) {
        if !recording {
            return;
        }

        if let Some(vad_arc) = vad {
            let mut det = vad_arc.lock().unwrap();
            match det.push_frame(samples).unwrap_or(VadFrame::Speech(samples)) {
                VadFrame::Speech(buf) => out_buf.extend_from_slice(buf),
                VadFrame::Noise => {}
            }
        } else {
            out_buf.extend_from_slice(samples);
        }
    }

    fn drain_loopback(
        lb_rx: &mpsc::Receiver<AudioChunk>,
        lb_resampler: &mut FrameResampler,
        lb_16k_buf: &mut Vec<f32>,
    ) {
        while let Ok(chunk) = lb_rx.try_recv() {
            if let AudioChunk::Samples(raw) = chunk {
                lb_resampler.push(&raw, &mut |frame: &[f32]| {
                    lb_16k_buf.extend_from_slice(frame);
                });
            }
        }
    }

    fn mix_and_feed(
        mic_buf: &mut Vec<f32>,
        lb_buf: &mut Vec<f32>,
        frame_buf: &mut Vec<f32>,
        recording: bool,
        vad: &Option<Arc<Mutex<Box<dyn vad::VoiceActivityDetector>>>>,
        out: &mut Vec<f32>,
    ) {
        let mix_len = mic_buf.len().min(lb_buf.len());
        for i in 0..mix_len {
            mic_buf[i] = (mic_buf[i] + lb_buf[i]).clamp(-1.0, 1.0);
        }
        if lb_buf.len() > mix_len {
            mic_buf.extend_from_slice(&lb_buf[mix_len..]);
        }
        lb_buf.clear();

        while mic_buf.len() >= FRAME_16K {
            frame_buf.clear();
            frame_buf.extend(mic_buf.drain(..FRAME_16K));
            handle_frame(frame_buf, recording, vad, out);
        }
    }

    loop {
        let chunk = match sample_rx.recv() {
            Ok(c) => c,
            Err(_) => break, // stream closed
        };

        let raw = match chunk {
            AudioChunk::Samples(s) => s,
            AudioChunk::EndOfStream => continue,
        };

        // ---------- spectrum processing (mic only) ----------------------- //
        if let Some(buckets) = visualizer.feed(&raw) {
            if let Some(cb) = &level_cb {
                cb(buckets);
            }
        }

        // ---------- audio pipeline --------------------------------------- //
        if has_loopback {
            frame_resampler.push(&raw, &mut |frame: &[f32]| {
                mic_16k_buf.extend_from_slice(frame);
            });
            if let (Some(ref lb_rx), Some(ref mut lb_res)) = (&loopback_rx, &mut lb_resampler) {
                drain_loopback(lb_rx, lb_res, &mut lb_16k_buf);
            }
            mix_and_feed(
                &mut mic_16k_buf,
                &mut lb_16k_buf,
                &mut frame_buf,
                recording,
                &vad,
                &mut processed_samples,
            );
        } else {
            frame_resampler.push(&raw, &mut |frame: &[f32]| {
                handle_frame(frame, recording, &vad, &mut processed_samples)
            });
        }

        // non-blocking check for a command
        while let Ok(cmd) = cmd_rx.try_recv() {
            match cmd {
                Cmd::Start => {
                    stop_flag.store(false, Ordering::Relaxed);
                    processed_samples.clear();
                    mic_16k_buf.clear();
                    lb_16k_buf.clear();
                    recording = true;
                    visualizer.reset();
                    if let Some(v) = &vad {
                        v.lock().unwrap().reset();
                    }
                }
                Cmd::Stop(reply_tx) => {
                    recording = false;
                    stop_flag.store(true, Ordering::Relaxed);

                    // Drain remaining mic audio
                    loop {
                        match sample_rx.recv_timeout(Duration::from_secs(2)) {
                            Ok(AudioChunk::Samples(remaining)) => {
                                if has_loopback {
                                    frame_resampler.push(&remaining, &mut |frame: &[f32]| {
                                        mic_16k_buf.extend_from_slice(frame);
                                    });
                                } else {
                                    frame_resampler.push(&remaining, &mut |frame: &[f32]| {
                                        handle_frame(frame, true, &vad, &mut processed_samples)
                                    });
                                }
                            }
                            Ok(AudioChunk::EndOfStream) => break,
                            Err(_) => {
                                log::warn!("Timed out waiting for EndOfStream from mic callback");
                                break;
                            }
                        }
                    }

                    // Drain remaining loopback audio
                    if let (Some(ref lb_rx), Some(ref mut lb_res)) =
                        (&loopback_rx, &mut lb_resampler)
                    {
                        loop {
                            match lb_rx.recv_timeout(Duration::from_secs(2)) {
                                Ok(AudioChunk::Samples(remaining)) => {
                                    lb_res.push(&remaining, &mut |frame: &[f32]| {
                                        lb_16k_buf.extend_from_slice(frame);
                                    });
                                }
                                Ok(AudioChunk::EndOfStream) => break,
                                Err(_) => {
                                    log::warn!(
                                        "Timed out waiting for EndOfStream from loopback callback"
                                    );
                                    break;
                                }
                            }
                        }
                    }

                    // Flush resamplers
                    if has_loopback {
                        frame_resampler.finish(&mut |frame: &[f32]| {
                            mic_16k_buf.extend_from_slice(frame);
                        });
                        if let Some(ref mut lb_res) = lb_resampler {
                            lb_res.finish(&mut |frame: &[f32]| {
                                lb_16k_buf.extend_from_slice(frame);
                            });
                        }
                        // Final mix of any remaining buffered audio
                        mix_and_feed(
                            &mut mic_16k_buf,
                            &mut lb_16k_buf,
                            &mut frame_buf,
                            true,
                            &vad,
                            &mut processed_samples,
                        );
                        // Feed any leftover sub-frame samples
                        if !mic_16k_buf.is_empty() {
                            handle_frame(&mic_16k_buf, true, &vad, &mut processed_samples);
                            mic_16k_buf.clear();
                        }
                    } else {
                        frame_resampler.finish(&mut |frame: &[f32]| {
                            handle_frame(frame, true, &vad, &mut processed_samples)
                        });
                    }

                    let _ = reply_tx.send(std::mem::take(&mut processed_samples));

                    // Resume the audio callback so the consumer loop can continue
                    // receiving chunks (important for always-on microphone mode).
                    stop_flag.store(false, Ordering::Relaxed);
                }
                Cmd::Shutdown => {
                    stop_flag.store(true, Ordering::Relaxed);
                    return;
                }
            }
        }
    }
}

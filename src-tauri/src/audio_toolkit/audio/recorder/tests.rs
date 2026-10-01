use super::{
    is_microphone_access_denied, is_no_input_device_error, run_consumer, AudioRecorder,
    CaptureProcessor, CaptureTransportState, ChunkDisposition, Cmd, VadConfig, VadPolicy,
};
use crate::audio_toolkit::vad::{VadFrame, VoiceActivityDetector};
use rtrb::RingBuffer;
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

#[test]
fn unopened_recorder_does_not_need_reopen() {
    let recorder = AudioRecorder::new().expect("recorder");
    assert!(!recorder.needs_reopen());
}

#[test]
fn stream_error_requires_reopen() {
    let recorder = AudioRecorder::new().expect("recorder");
    recorder.stream_error.store(true, Ordering::Relaxed);
    assert!(recorder.needs_reopen());
}

/// Pass-through detector with a configurable frame size, standing in for a
/// backend such as Earshot whose frames are not 30 ms.
struct FixedFrameVad(usize);

impl VoiceActivityDetector for FixedFrameVad {
    fn push_frame<'a>(&'a mut self, frame: &'a [f32]) -> anyhow::Result<VadFrame<'a>> {
        Ok(VadFrame::Speech(frame))
    }

    fn frame_samples(&self) -> usize {
        self.0
    }
}

#[test]
fn resampler_frame_size_follows_the_vad_backend() {
    let frame_samples = 256;
    let vad = VadConfig {
        detector: Arc::new(Mutex::new(Box::new(FixedFrameVad(frame_samples)))),
        frame_samples,
        offline_hangover_frames: 0,
        streaming_hangover_frames: 0,
    };
    let frame_lengths = Arc::new(Mutex::new(Vec::new()));
    let observed = Arc::clone(&frame_lengths);
    let mut processor = CaptureProcessor::new(
        16_000,
        Some(vad),
        None,
        Some(Arc::new(move |frame: &[f32]| {
            observed.lock().unwrap().push(frame.len())
        })),
        Instant::now(),
    );

    let (ready_tx, _ready_rx) = mpsc::channel();
    processor.begin_recording(VadPolicy::Offline, ready_tx);
    processor.process_raw_chunk(&[0.0; 1024], ChunkDisposition::Capture);
    let samples = processor.finish_recording().samples;

    assert_eq!(samples.len(), 1024);
    assert_eq!(*frame_lengths.lock().unwrap(), vec![frame_samples; 4]);
}

#[test]
fn idle_chunks_are_discarded_without_reaching_the_recording() {
    let mut processor = CaptureProcessor::new(16_000, None, None, None, Instant::now());
    processor.process_raw_chunk(&[1.0; 480], ChunkDisposition::Discard);
    assert!(processor.finish_recording().samples.is_empty());
}

#[test]
fn shutdown_is_processed_without_audio_samples() {
    let (_producer, consumer) = RingBuffer::<f32>::new(48_000);
    let (cmd_tx, cmd_rx) = mpsc::channel();
    let (done_tx, done_rx) = mpsc::channel();
    let worker = thread::spawn(move || {
        run_consumer(
            CaptureProcessor::new(48_000, None, None, None, Instant::now()),
            consumer,
            cmd_rx,
            Arc::new(CaptureTransportState::default()),
            Arc::new(AtomicBool::new(false)),
        );
        let _ = done_tx.send(());
    });

    cmd_tx.send(Cmd::Shutdown).expect("send shutdown");
    assert!(done_rx.recv_timeout(Duration::from_secs(1)).is_ok());
    worker.join().expect("join consumer");
}

#[test]
fn callback_writes_mono_samples() {
    let (mut producer, mut consumer) = RingBuffer::<f32>::new(8);
    let transport = CaptureTransportState::default();

    AudioRecorder::write_input_to_ring(&[0.25f32, -0.5, 1.0], 1, None, &mut producer, &transport);

    let mut output = [0.0; 3];
    consumer.pop_entire_slice(&mut output).expect("samples");
    assert_eq!(output, [0.25, -0.5, 1.0]);
}

#[test]
fn callback_downmixes_or_selects_multichannel_input() {
    let transport = CaptureTransportState::default();
    let (mut average_tx, mut average_rx) = RingBuffer::<f32>::new(4);
    AudioRecorder::write_input_to_ring(
        &[1.0f32, 3.0, -1.0, 1.0],
        2,
        None,
        &mut average_tx,
        &transport,
    );
    let mut averaged = [0.0; 2];
    average_rx
        .pop_entire_slice(&mut averaged)
        .expect("averaged samples");
    assert_eq!(averaged, [2.0, 0.0]);

    let (mut selected_tx, mut selected_rx) = RingBuffer::<f32>::new(4);
    AudioRecorder::write_input_to_ring(
        &[1.0f32, 3.0, -1.0, 1.0],
        2,
        Some(1),
        &mut selected_tx,
        &transport,
    );
    let mut selected = [0.0; 2];
    selected_rx
        .pop_entire_slice(&mut selected)
        .expect("selected samples");
    assert_eq!(selected, [3.0, 1.0]);
}

#[test]
fn callback_forwards_boundary_block_then_stays_silent_until_resumed() {
    let (mut producer, mut consumer) = RingBuffer::<f32>::new(8);
    let transport = CaptureTransportState::default();

    // The block in hand when a pause is first observed was captured before
    // the stop, so it is forwarded and only then acknowledged.
    transport.pause_requested.store(true, Ordering::Release);
    AudioRecorder::write_input_to_ring(&[1.0f32, 2.0], 1, None, &mut producer, &transport);
    assert!(transport.pause_acknowledged.load(Ordering::Acquire));
    assert_eq!(consumer.slots(), 2);

    // Later blocks while paused are dropped and are not counted as overruns.
    AudioRecorder::write_input_to_ring(&[3.0f32], 1, None, &mut producer, &transport);
    assert_eq!(consumer.slots(), 2);
    assert_eq!(transport.overrun_samples.load(Ordering::Relaxed), 0);

    // Clearing the pause, as the consumer does before stop() returns, resumes capture.
    transport.pause_acknowledged.store(false, Ordering::Relaxed);
    transport.pause_requested.store(false, Ordering::Release);
    AudioRecorder::write_input_to_ring(&[4.0f32], 1, None, &mut producer, &transport);
    let mut output = [0.0; 3];
    consumer.pop_entire_slice(&mut output).expect("samples");
    assert_eq!(output, [1.0, 2.0, 4.0]);
    assert!(!transport.pause_acknowledged.load(Ordering::Acquire));
}

#[test]
fn callback_partially_fills_ring_and_counts_dropped_audio() {
    let (mut producer, mut consumer) = RingBuffer::<f32>::new(2);
    let transport = CaptureTransportState::default();

    AudioRecorder::write_input_to_ring(&[1.0f32, 2.0, 3.0], 1, None, &mut producer, &transport);

    let mut captured = [0.0; 2];
    consumer
        .pop_entire_slice(&mut captured)
        .expect("partial callback audio");
    assert_eq!(captured, [1.0, 2.0]);
    assert_eq!(transport.overrun_samples.load(Ordering::Relaxed), 1);
}

#[test]
fn bounded_drain_leaves_remaining_samples_for_the_next_command_cycle() {
    let (mut producer, mut consumer) = RingBuffer::<f32>::new(8);
    producer
        .push_entire_slice(&[1.0, 2.0, 3.0, 4.0, 5.0])
        .expect("samples");
    let mut drained = Vec::new();

    let count =
        super::drain_available_samples(&mut consumer, 3, |part| drained.extend_from_slice(part));

    assert_eq!(count, 3);
    assert_eq!(drained, [1.0, 2.0, 3.0]);
    assert_eq!(consumer.slots(), 2);
}

#[test]
fn ring_wraparound_preserves_both_read_slices_in_order() {
    let (mut producer, mut consumer) = RingBuffer::<f32>::new(5);
    let transport = CaptureTransportState::default();
    producer
        .push_entire_slice(&[1.0, 2.0, 3.0, 4.0])
        .expect("initial samples");
    let mut discarded = [0.0; 3];
    consumer
        .pop_entire_slice(&mut discarded)
        .expect("advance ring head");

    AudioRecorder::write_input_to_ring(
        &[5.0f32, 6.0, 7.0, 8.0],
        1,
        None,
        &mut producer,
        &transport,
    );

    let chunk = consumer.read_chunk(5).expect("wrapped samples");
    let (first, second) = chunk.as_slices();
    assert!(!first.is_empty());
    assert!(!second.is_empty());
    let ordered = first
        .iter()
        .chain(second.iter())
        .copied()
        .collect::<Vec<_>>();
    assert_eq!(ordered, [4.0, 5.0, 6.0, 7.0, 8.0]);
}

#[test]
fn repeated_start_stop_cycles_resume_capture_without_leaking_samples() {
    let (mut producer, consumer) = RingBuffer::<f32>::new(16_000);
    let transport = Arc::new(CaptureTransportState::default());
    let (cmd_tx, cmd_rx) = mpsc::channel();
    let streamed = Arc::new(Mutex::new(Vec::new()));
    let streamed_cb = Arc::clone(&streamed);
    let consumer_transport = Arc::clone(&transport);
    let worker = thread::spawn(move || {
        let processor = CaptureProcessor::new(
            16_000,
            None,
            None,
            Some(Arc::new(move |frame: &[f32]| {
                streamed_cb.lock().unwrap().extend_from_slice(frame)
            })),
            Instant::now(),
        );
        run_consumer(
            processor,
            consumer,
            cmd_rx,
            consumer_transport,
            Arc::new(AtomicBool::new(false)),
        );
    });

    let wait_for_pause_request = || {
        let deadline = Instant::now() + Duration::from_secs(1);
        while !transport.pause_requested.load(Ordering::Acquire) {
            assert!(Instant::now() < deadline, "pause was not requested");
            thread::sleep(Duration::from_millis(1));
        }
    };

    let first_input = [0.25f32, -0.5, 1.0];
    let (ready_tx, ready_rx) = mpsc::channel();
    cmd_tx
        .send(Cmd::Start(VadPolicy::Disabled, Instant::now(), ready_tx))
        .expect("first start");
    AudioRecorder::write_input_to_ring(&first_input, 1, None, &mut producer, &transport);
    ready_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("first capture ready");

    let (reply_tx, reply_rx) = mpsc::channel();
    cmd_tx.send(Cmd::Stop(reply_tx)).expect("first stop");
    wait_for_pause_request();
    // The first callback after Stop carries audio captured before the stop,
    // so it belongs to the recording.
    AudioRecorder::write_input_to_ring(&[99.0f32], 1, None, &mut producer, &transport);

    let first_samples = reply_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("first stop reply")
        .samples;
    let first_expected = [0.25f32, -0.5, 1.0, 99.0];
    assert_eq!(&first_samples[..first_expected.len()], &first_expected);
    assert!(first_samples[first_expected.len()..]
        .iter()
        .all(|&sample| sample == 0.0));
    assert!(!transport.pause_requested.load(Ordering::Acquire));

    let first_streamed_len = {
        let streamed = streamed.lock().unwrap();
        assert_eq!(&streamed[..first_expected.len()], &first_expected);
        streamed.len()
    };

    // Start again immediately after stop() would have returned. The producer
    // must already be re-enabled, and no first-cycle samples may leak through.
    let second_input = [0.75f32, -0.25, 0.5];
    let (ready_tx, ready_rx) = mpsc::channel();
    cmd_tx
        .send(Cmd::Start(VadPolicy::Disabled, Instant::now(), ready_tx))
        .expect("second start");
    AudioRecorder::write_input_to_ring(&second_input, 1, None, &mut producer, &transport);
    ready_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("second capture ready");

    let (reply_tx, reply_rx) = mpsc::channel();
    cmd_tx.send(Cmd::Stop(reply_tx)).expect("second stop");
    wait_for_pause_request();
    AudioRecorder::write_input_to_ring(&[199.0f32], 1, None, &mut producer, &transport);

    let second_samples = reply_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("second stop reply")
        .samples;
    let second_expected = [0.75f32, -0.25, 0.5, 199.0];
    assert_eq!(&second_samples[..second_expected.len()], &second_expected);
    assert!(second_samples[second_expected.len()..]
        .iter()
        .all(|&sample| sample == 0.0));
    assert!(!first_samples
        .iter()
        .any(|sample| second_expected.contains(sample)));
    assert!(!second_samples
        .iter()
        .any(|sample| first_expected.contains(sample)));
    assert!(!transport.pause_requested.load(Ordering::Acquire));

    {
        let streamed = streamed.lock().unwrap();
        assert_eq!(streamed.len(), first_streamed_len + second_samples.len());
        assert_eq!(
            &streamed[first_streamed_len..first_streamed_len + second_expected.len()],
            &second_expected
        );
    }

    cmd_tx.send(Cmd::Shutdown).expect("shutdown");
    worker.join().expect("consumer worker");
}

#[test]
fn missing_callback_at_stop_marks_stream_for_rebuild_and_returns_samples() {
    let (_producer, consumer) = RingBuffer::<f32>::new(16_000);
    let transport = Arc::new(CaptureTransportState::default());
    let stream_error = Arc::new(AtomicBool::new(false));
    let observed_error = Arc::clone(&stream_error);
    let (cmd_tx, cmd_rx) = mpsc::channel();
    let worker_transport = Arc::clone(&transport);
    let worker = thread::spawn(move || {
        run_consumer(
            CaptureProcessor::new(16_000, None, None, None, Instant::now()),
            consumer,
            cmd_rx,
            worker_transport,
            stream_error,
        );
    });

    let (ready_tx, _ready_rx) = mpsc::channel();
    cmd_tx
        .send(Cmd::Start(VadPolicy::Disabled, Instant::now(), ready_tx))
        .expect("start");
    let (reply_tx, reply_rx) = mpsc::channel();
    cmd_tx.send(Cmd::Stop(reply_tx)).expect("stop");

    let samples = reply_rx
        .recv_timeout(Duration::from_secs(3))
        .expect("pause timeout still returns captured samples")
        .samples;
    assert!(samples.is_empty());
    worker.join().expect("consumer exits after pause timeout");
    assert!(observed_error.load(Ordering::Acquire));
}

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

struct TestMicrophoneCapture {
    producer: rtrb::Producer<f32>,
    commands: mpsc::Sender<Cmd>,
    levels: mpsc::Receiver<f32>,
    frames: mpsc::Receiver<Vec<f32>>,
    transport: Arc<CaptureTransportState>,
    stream_error: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}

impl TestMicrophoneCapture {
    fn new(sample_rate: u32) -> Self {
        let (producer, consumer) = RingBuffer::new(sample_rate as usize * 2);
        let (commands, command_rx) = mpsc::channel();
        let (level_tx, levels) = mpsc::channel();
        let (frame_tx, frames) = mpsc::channel();
        let transport = Arc::new(CaptureTransportState::default());
        let stream_error = Arc::new(AtomicBool::new(false));
        let worker_transport = transport.clone();
        let worker_error = stream_error.clone();
        let worker = thread::spawn(move || {
            let mut processor = CaptureProcessor::new(
                sample_rate,
                None,
                None,
                Some(Arc::new(move |frame| {
                    let _ = frame_tx.send(frame.to_vec());
                })),
                Instant::now(),
            );
            processor.microphone_test_level_cb = Some(Arc::new(move |level| {
                let _ = level_tx.send(level);
            }));
            run_consumer(
                processor,
                consumer,
                command_rx,
                worker_transport,
                worker_error,
            );
        });
        Self {
            producer,
            commands,
            levels,
            frames,
            transport,
            stream_error,
            worker: Some(worker),
        }
    }

    fn feed(&mut self, samples: &[f32]) {
        AudioRecorder::write_input_to_ring(samples, 1, None, &mut self.producer, &self.transport);
    }
}

impl Drop for TestMicrophoneCapture {
    fn drop(&mut self) {
        let _ = self.commands.send(Cmd::Shutdown);
        if let Some(worker) = self.worker.take() {
            worker.join().unwrap();
        }
    }
}

#[test]
fn microphone_test_emits_levels_until_stopped() {
    let mut capture = TestMicrophoneCapture::new(30);
    capture.commands.send(Cmd::StartMicrophoneTest).unwrap();
    capture.feed(&[0.1]);
    let level = capture.levels.recv_timeout(Duration::from_secs(1)).unwrap();
    assert!((level - (2.0 / 3.0)).abs() < 0.001);
    assert!(
        capture.frames.try_recv().is_err(),
        "test audio reached transcription"
    );
    let (stop_tx, stop_rx) = mpsc::channel();
    capture
        .commands
        .send(Cmd::StopMicrophoneTest(stop_tx))
        .unwrap();
    stop_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    capture.feed(&[0.1]);
    assert!(capture
        .levels
        .recv_timeout(Duration::from_millis(50))
        .is_err());
    assert!(capture.frames.try_recv().is_err());
}

#[test]
fn recording_start_preempts_microphone_test() {
    let mut capture = TestMicrophoneCapture::new(16_000);
    capture.commands.send(Cmd::StartMicrophoneTest).unwrap();
    capture.feed(&[0.1; 960]);
    capture.levels.recv_timeout(Duration::from_secs(1)).unwrap();
    let (ready_tx, ready_rx) = mpsc::channel();
    capture
        .commands
        .send(Cmd::Start(VadPolicy::Disabled, Instant::now(), ready_tx))
        .unwrap();
    capture.feed(&[0.2; 960]);
    ready_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    assert!(capture
        .levels
        .recv_timeout(Duration::from_millis(50))
        .is_err());
    assert!(capture.frames.recv_timeout(Duration::from_secs(1)).is_ok());
}

#[test]
fn microphone_test_exits_when_capture_fails_without_more_samples() {
    let mut capture = TestMicrophoneCapture::new(30);
    capture.commands.send(Cmd::StartMicrophoneTest).unwrap();
    capture.feed(&[0.1]);
    capture.levels.recv_timeout(Duration::from_secs(1)).unwrap();
    // CPAL can stop sending samples while keeping the ring producer alive.
    capture.stream_error.store(true, Ordering::Release);
    // The level callback sender belongs to the consumer processor. Dropping it
    // proves the consumer ended, so the owning worker can drop the CPAL stream.
    assert!(matches!(
        capture.levels.recv_timeout(Duration::from_secs(1)),
        Err(mpsc::RecvTimeoutError::Disconnected)
    ));
}

struct RejectAllAudio;
impl VoiceActivityDetector for RejectAllAudio {
    fn frame_samples(&self) -> usize {
        480
    }
    fn push_frame<'a>(&'a mut self, _frame: &'a [f32]) -> anyhow::Result<VadFrame<'a>> {
        Ok(VadFrame::Noise)
    }
}

fn capture_rejected_input(input: &[f32], drained: &[f32]) -> super::CapturedAudio {
    let (mut producer, consumer) = RingBuffer::new(16_000);
    let (command_tx, command_rx) = mpsc::channel();
    let transport = Arc::new(CaptureTransportState::default());
    let worker_transport = transport.clone();
    let worker = thread::spawn(move || {
        let processor = CaptureProcessor::new(
            16_000,
            Some(VadConfig {
                detector: Arc::new(Mutex::new(Box::new(RejectAllAudio))),
                frame_samples: 480,
                offline_hangover_frames: 0,
                streaming_hangover_frames: 0,
            }),
            None,
            None,
            Instant::now(),
        );
        run_consumer(
            processor,
            consumer,
            command_rx,
            worker_transport,
            Arc::new(AtomicBool::new(false)),
        );
    });
    let (ready_tx, ready_rx) = mpsc::channel();
    command_tx
        .send(Cmd::Start(VadPolicy::Offline, Instant::now(), ready_tx))
        .unwrap();
    AudioRecorder::write_input_to_ring(input, 1, None, &mut producer, &transport);
    if !input.is_empty() {
        ready_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    }
    let (reply_tx, reply_rx) = mpsc::channel();
    command_tx.send(Cmd::Stop(reply_tx)).unwrap();
    let deadline = Instant::now() + Duration::from_secs(1);
    while !transport.pause_requested.load(Ordering::Acquire) {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(1));
    }
    AudioRecorder::write_input_to_ring(drained, 1, None, &mut producer, &transport);
    let captured = reply_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    command_tx.send(Cmd::Shutdown).unwrap();
    worker.join().unwrap();
    assert!(
        captured.samples.is_empty(),
        "the VAD must actually reject this input"
    );
    captured
}

#[test]
fn nonzero_input_rejected_by_vad_is_not_silent() {
    assert!(
        !capture_rejected_input(&[0.2; 960], &[]).input_is_silent,
        "nonzero Noise was misclassified as silent input"
    );
}

#[test]
fn silent_input_remains_silent_after_vad_discards_it() {
    for input in [vec![], vec![0.0; 960], vec![0.0005; 960]] {
        assert!(capture_rejected_input(&input, &[]).input_is_silent);
    }
}

#[test]
fn nonzero_input_in_the_stop_drain_is_not_silent() {
    assert!(!capture_rejected_input(&[0.0; 960], &[0.2; 960]).input_is_silent);
}

#[test]
fn microphone_test_audio_is_not_retained_or_classified_as_recording_input() {
    let (frame_tx, frame_rx) = mpsc::channel();
    let mut processor = CaptureProcessor::new(
        16_000,
        None,
        None,
        Some(Arc::new(move |frame| {
            frame_tx.send(frame.to_vec()).unwrap()
        })),
        Instant::now(),
    );
    processor.process_raw_chunk(&[0.2; 960], ChunkDisposition::MicrophoneTest);
    let captured = processor.finish_recording();
    assert!(captured.samples.is_empty());
    assert!(captured.input_is_silent);
    assert!(frame_rx.try_recv().is_err());
}

#[test]
fn input_silence_is_reset_between_recordings() {
    let mut processor = CaptureProcessor::new(16_000, None, None, None, Instant::now());
    let (ready_tx, _) = mpsc::channel();
    processor.begin_recording(VadPolicy::Disabled, ready_tx);
    processor.process_raw_chunk(&[0.2; 480], ChunkDisposition::Capture);
    assert!(!processor.finish_recording().input_is_silent);
    let (ready_tx, _) = mpsc::channel();
    processor.begin_recording(VadPolicy::Disabled, ready_tx);
    processor.process_raw_chunk(&[0.2; 480], ChunkDisposition::Discard);
    processor.process_raw_chunk(&[0.0; 480], ChunkDisposition::Capture);
    assert!(processor.finish_recording().input_is_silent);
}

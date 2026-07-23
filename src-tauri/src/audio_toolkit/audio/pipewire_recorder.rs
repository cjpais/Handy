//! Native PipeWire microphone capture backend (Linux-only).
//!
//! Why this exists: the cpal path pins capture to the ALSA host
//! (`get_cpal_host()` -> `HostId::Alsa`), so Handy's mic stream is an ALSA
//! "default" client that PipeWire cannot see as a first-class node — no per-app
//! volume, no routing, no node selection. This backend registers Handy's
//! capture as a real PipeWire node instead, so it shows up in `wpctl status`
//! with its own settable volume and is routable per-app.
//!
//! Threading / ownership model (the important part):
//!   * PipeWire objects (MainLoop, Context, Core, Stream, listeners) are NOT
//!     `Send`. We therefore construct and own ALL of them on ONE dedicated
//!     thread (`run_pipewire_loop`) and never move them off it. Cross-thread
//!     communication happens only through channels and the SPSC audio ring.
//!   * A SECOND thread runs the backend-neutral `run_consumer` (drain -> resample
//!     -> VAD -> buffer) — the exact same consumer the cpal recorder uses. The RT
//!     `process` callback (on PipeWire's data thread) downmixes to mono straight
//!     into the ring's `Producer<f32>`, mirroring `write_input_to_ring`. This is
//!     the seam that lets us reuse the whole pipeline below the mono-`f32`
//!     producer without reimplementing any of it.
//!   * Pause/overrun handshakes go through the SHARED `CaptureTransportState`,
//!     so a stop drains the identical way it does on cpal.
//!   * Start/Stop/Shutdown reuse the shared `Cmd` protocol: `cmd_tx` talks to
//!     `run_consumer` identically to the cpal backend. A separate
//!     `pipewire::channel` sender wakes the loop thread to quit it on `close()`.
//!
//! The `process` callback must stay allocation-, lock-, logging- and
//! blocking-free, exactly like the cpal one.
//!
//! Targets the `pipewire` crate 0.10 with the `v0_3_44` feature (for
//! `TARGET_OBJECT`). Non-obvious calls are commented inline.

use std::io::Cursor;
use std::mem;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc,
};
use std::time::Instant;

use pipewire as pw;
use pw::{properties::properties, spa};
use rtrb::{Producer, RingBuffer};

use spa::param::audio::{AudioFormat, AudioInfoRaw};
use spa::param::format::{MediaSubtype, MediaType};
use spa::param::format_utils;
use spa::param::ParamType;
use spa::pod::{serialize::PodSerializer, Object, Pod, Value};
use spa::utils::{Direction, SpaTypes};

use super::recorder::{
    acknowledge_pause_after_write, run_consumer, AudioFrameCallback, CaptureProcessor,
    CaptureTransportState, Cmd, LevelCallback, VadConfig, AUDIO_RING_SECONDS,
};
use super::VadPolicy;

/// Human-readable identity for the capture node in `wpctl status`.
const APP_NAME: &str = "Handy";
const NODE_NAME: &str = "handy-capture";

/// We pin the negotiated capture rate so the consumer's `FrameResampler`
/// (rate -> 16 kHz) can be constructed up front, before PipeWire's async format
/// negotiation completes. PipeWire's adapter resamples the source to this rate
/// for us. 48 kHz is the near-universal graph rate, so this is usually a no-op
/// conversion. Channels are left unpinned and downmixed in `process`.
const PIPEWIRE_CAPTURE_RATE: u32 = 48_000;

/// Bytes per interleaved sample; we always negotiate F32LE.
const SAMPLE_STRIDE: usize = mem::size_of::<f32>();

/// Native PipeWire capture backend. Public surface intentionally mirrors the
/// cpal `AudioRecorder` (`from_parts`/`open`/`start`/`stop`/`close`) so the
/// `Recorder` seam can drive either backend the same way.
pub struct PipeWireRecorder {
    /// Shared VAD + callbacks handed to `run_consumer`. Cloned per `open`.
    vad: Option<VadConfig>,
    level_cb: Option<LevelCallback>,
    audio_cb: Option<AudioFrameCallback>,

    /// Talks to `run_consumer` (Start/Stop/Shutdown) — same protocol as cpal.
    cmd_tx: Option<mpsc::Sender<Cmd>>,
    /// Wakes the loop thread to quit it on `close()`. `pipewire::channel`'s
    /// sender is `Send`; its receiver is attached to the (non-`Send`) loop.
    quit_tx: Option<pw::channel::Sender<()>>,
    /// Set when the PipeWire stream reports an error state, or when the shared
    /// consumer gives up on a pause handshake. Mirrors the cpal backend's
    /// `stream_error` so `Recorder::needs_reopen` works on both paths.
    stream_error: Arc<AtomicBool>,
    /// The dedicated PipeWire loop thread and the consumer thread.
    pw_handle: Option<std::thread::JoinHandle<()>>,
    consumer_handle: Option<std::thread::JoinHandle<()>>,
}

impl PipeWireRecorder {
    /// Build from already-shared parts. See `AudioRecorder::from_parts`.
    pub(crate) fn from_parts(
        vad: Option<VadConfig>,
        level_cb: Option<LevelCallback>,
        audio_cb: Option<AudioFrameCallback>,
    ) -> Self {
        PipeWireRecorder {
            vad,
            level_cb,
            audio_cb,
            cmd_tx: None,
            quit_tx: None,
            stream_error: Arc::new(AtomicBool::new(false)),
            pw_handle: None,
            consumer_handle: None,
        }
    }

    /// Open the capture stream. `target_node` optionally pins capture to a
    /// specific source by its `node.name` (via `TARGET_OBJECT`); `None`
    /// autoconnects to the system default source — reproducing today's cpal
    /// "default" behaviour.
    ///
    /// Returns `Err` if the PipeWire connection/stream setup fails (e.g. no
    /// PipeWire session running), which is the signal the `Recorder` seam uses
    /// to fall back to the cpal/ALSA backend (or to report a hard failure when
    /// the user pinned the PipeWire backend explicitly).
    pub fn open(&mut self, target_node: Option<String>) -> Result<(), Box<dyn std::error::Error>> {
        if self.pw_handle.is_some() {
            return Ok(()); // already open
        }

        self.stream_error.store(false, Ordering::Relaxed);

        // Wait-free SPSC audio ring, sized exactly like the cpal backend's.
        // Built here (not on the pw thread) because the rate is pinned up front.
        let ring_capacity = PIPEWIRE_CAPTURE_RATE as usize * AUDIO_RING_SECONDS;
        let (mut sample_producer, mut sample_consumer) = RingBuffer::<f32>::new(ring_capacity);

        // Touch rtrb's uninitialized pages before the stream starts to reduce
        // callback page faults, mirroring `AudioRecorder::build_stream`.
        {
            let chunk = sample_producer
                .write_chunk(ring_capacity)
                .expect("new audio ring has its full capacity available");
            chunk.commit_all();
        }
        {
            let chunk = sample_consumer
                .read_chunk(ring_capacity)
                .expect("pre-filled audio ring is readable");
            chunk.commit_all();
        }

        // Control channel to run_consumer (Start/Stop/Shutdown).
        let (cmd_tx, cmd_rx) = mpsc::channel::<Cmd>();
        // One-shot init handshake so open() can report setup success/failure.
        let (init_tx, init_rx) = mpsc::sync_channel::<Result<(), String>>(1);
        // Cross-thread wake to quit the loop on close(). Attached to the loop
        // inside the pipewire thread; sender kept here.
        let (quit_tx, quit_rx) = pw::channel::channel::<()>();

        // Shared pause/overrun protocol between the RT callback and the consumer.
        let transport = Arc::new(CaptureTransportState::default());

        // ---- consumer thread: the reused, backend-neutral pipeline --------- //
        let processor = CaptureProcessor::new(
            PIPEWIRE_CAPTURE_RATE,
            self.vad.clone(),
            self.level_cb.clone(),
            self.audio_cb.clone(),
            Instant::now(),
        );
        let consumer_transport = Arc::clone(&transport);
        let consumer_stream_error = Arc::clone(&self.stream_error);
        let consumer_handle = std::thread::spawn(move || {
            run_consumer(
                processor,
                sample_consumer,
                cmd_rx,
                consumer_transport,
                consumer_stream_error,
            );
        });

        // ---- pipewire loop thread: owns all non-Send pw objects ------------ //
        let pw_stream_error = Arc::clone(&self.stream_error);
        let pw_handle = std::thread::spawn(move || {
            run_pipewire_loop(
                sample_producer,
                transport,
                pw_stream_error,
                target_node,
                init_tx,
                quit_rx,
            );
        });

        match init_rx.recv() {
            Ok(Ok(())) => {
                self.cmd_tx = Some(cmd_tx);
                self.quit_tx = Some(quit_tx);
                self.pw_handle = Some(pw_handle);
                self.consumer_handle = Some(consumer_handle);
                Ok(())
            }
            Ok(Err(error_message)) => {
                // Setup failed: the pw thread has already returned. Dropping
                // `cmd_tx` disconnects the consumer's command channel, which is
                // its termination signal. Join both before reporting.
                drop(cmd_tx);
                let _ = pw_handle.join();
                let _ = consumer_handle.join();
                Err(Box::new(std::io::Error::other(error_message)))
            }
            Err(recv_error) => {
                drop(cmd_tx);
                let _ = pw_handle.join();
                let _ = consumer_handle.join();
                Err(Box::new(std::io::Error::other(format!(
                    "PipeWire capture worker died during init: {recv_error}"
                ))))
            }
        }
    }

    pub fn start(
        &self,
        vad_policy: VadPolicy,
    ) -> Result<mpsc::Receiver<()>, Box<dyn std::error::Error>> {
        // Mirror the cpal backend exactly: error if not open, else hand back the
        // one-shot receiver the shared `run_consumer` fires after the first
        // captured chunk, so the `Recorder` seam surfaces first-sample readiness
        // the same way for both backends.
        let tx = self
            .cmd_tx
            .as_ref()
            .ok_or_else(|| std::io::Error::other("PipeWire recorder is not open"))?;
        let (ready_tx, ready_rx) = mpsc::channel();
        tx.send(Cmd::Start(vad_policy, Instant::now(), ready_tx))?;
        Ok(ready_rx)
    }

    pub fn stop(&self) -> Result<Vec<f32>, Box<dyn std::error::Error>> {
        let (resp_tx, resp_rx) = mpsc::channel();
        if let Some(tx) = &self.cmd_tx {
            tx.send(Cmd::Stop(resp_tx))?;
        }
        Ok(resp_rx.recv()?) // wait for the buffered samples
    }

    pub fn close(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        // Best-effort clean shutdown of the consumer first…
        if let Some(tx) = self.cmd_tx.take() {
            let _ = tx.send(Cmd::Shutdown);
        }
        // …then quit the loop thread. Once it returns, the stream/listener and
        // the ring producer drop, which also unblocks the consumer if it hadn't
        // already seen Shutdown. Either path terminates both threads.
        if let Some(quit) = self.quit_tx.take() {
            let _ = quit.send(());
        }
        if let Some(h) = self.pw_handle.take() {
            let _ = h.join();
        }
        if let Some(h) = self.consumer_handle.take() {
            let _ = h.join();
        }
        Ok(())
    }

    /// Whether the stream died and must be rebuilt before the next recording.
    pub fn needs_reopen(&self) -> bool {
        self.stream_error.load(Ordering::Acquire)
    }
}

impl Drop for PipeWireRecorder {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

/// Per-stream state owned by the listener on the pipewire loop thread. Holds the
/// ring producer and the shared transport. Not `Send` is fine — it never leaves
/// the loop thread.
struct CaptureState {
    /// Negotiated raw-audio format; channels filled in by `param_changed`.
    format: AudioInfoRaw,
    /// Producer end of the SPSC ring drained by `run_consumer`.
    producer: Producer<f32>,
    /// Shared pause/overrun handshake with `run_consumer`.
    transport: Arc<CaptureTransportState>,
    /// Raised when the stream enters an error state.
    stream_error: Arc<AtomicBool>,
}

/// Body of the dedicated pipewire thread. Constructs the full MainLoop -> Core
/// -> Stream graph HERE (nothing pipewire crosses a thread boundary), reports
/// setup success/failure through `init_tx`, then blocks in `mainloop.run()`
/// until `close()` sends a quit.
fn run_pipewire_loop(
    producer: Producer<f32>,
    transport: Arc<CaptureTransportState>,
    stream_error: Arc<AtomicBool>,
    target_node: Option<String>,
    init_tx: mpsc::SyncSender<Result<(), String>>,
    quit_rx: pw::channel::Receiver<()>,
) {
    // All fallible setup runs in this closure so a single `?` chain can report
    // failure via init_tx (the seam's fallback trigger).
    let setup = (|| -> Result<(), pw::Error> {
        pw::init();

        // MainLoop drives this thread. 0.10 exposes the ref-counted variants.
        let mainloop = pw::main_loop::MainLoopRc::new(None)?;
        let context = pw::context::ContextRc::new(&mainloop, None)?;
        let core = context.connect_rc(None)?;

        // Attach the quit receiver to this loop. The callback runs ON the loop
        // thread, so it can safely stop the (non-Send) loop. Keep the returned
        // guard alive for the whole run — dropping it detaches the receiver.
        let _quit_guard = quit_rx.attach(mainloop.loop_(), {
            let mainloop = mainloop.clone();
            move |_| mainloop.quit()
        });

        // Advertise ourselves as a Communication-role audio capture stream with
        // a stable, human-readable identity. The Communication role matches a
        // voice-capture use case and lets session policy route it accordingly.
        let mut props = properties! {
            *pw::keys::MEDIA_TYPE => "Audio",
            *pw::keys::MEDIA_CATEGORY => "Capture",
            *pw::keys::MEDIA_ROLE => "Communication",
            *pw::keys::APP_NAME => APP_NAME,
            *pw::keys::NODE_NAME => NODE_NAME,
        };

        // Optionally pin to a specific source by node.name. TARGET_OBJECT is the
        // modern targeting property (needs the v0_3_44 feature); as a stream prop
        // it is equivalent to passing a target to connect().
        if let Some(target) = target_node {
            if !target.is_empty() {
                log::info!("PipeWire capture targeting node '{target}'");
                props.insert(*pw::keys::TARGET_OBJECT, target);
            }
        }

        let stream = pw::stream::StreamBox::new(&core, APP_NAME, props)?;

        // The listener owns our CaptureState; the callbacks get `&mut` to it.
        // `_listener` must outlive the run — dropping it unregisters callbacks.
        let _listener = stream
            .add_local_listener_with_user_data(CaptureState {
                format: AudioInfoRaw::default(),
                producer,
                transport,
                stream_error,
            })
            // Read back the server-chosen concrete format (we only pinned the
            // sample format + rate; channels are negotiated).
            .param_changed(|_stream, state, id, param| {
                let Some(param) = param else {
                    return;
                };
                if id != ParamType::Format.as_raw() {
                    return;
                }
                let Ok((media_type, media_subtype)) = format_utils::parse_format(param) else {
                    return;
                };
                if media_type != MediaType::Audio || media_subtype != MediaSubtype::Raw {
                    return;
                }
                if state.format.parse(param).is_err() {
                    log::warn!("PipeWire: failed to parse negotiated audio format");
                    return;
                }
                log::info!(
                    "PipeWire capture negotiated: rate={} Hz, channels={}",
                    state.format.rate(),
                    state.format.channels()
                );
            })
            // Surface a dead stream the same way cpal's error callback does, so
            // `needs_reopen` rebuilds it before the next recording.
            .state_changed(|_stream, state, _old, new| {
                if let pw::stream::StreamState::Error(message) = new {
                    log::error!("PipeWire capture stream error: {message}");
                    state.stream_error.store(true, Ordering::Release);
                }
            })
            // RT callback (on the data thread): downmix interleaved F32 frames
            // to mono directly into the ring. Keep this allocation-free,
            // wait-free, and free of locks, logging, clocks and system calls —
            // same contract as `AudioRecorder::write_input_to_ring`.
            .process(|stream, state| {
                let Some(mut buffer) = stream.dequeue_buffer() else {
                    return;
                };
                let datas = buffer.datas_mut();
                if datas.is_empty() {
                    return;
                }

                // Forward the first block that observes a pause; once
                // acknowledged, stay silent until the consumer resumes capture.
                if state.transport.is_capture_paused() {
                    return;
                }

                let channels = state.format.channels().max(1) as usize;
                let data = &mut datas[0];
                // chunk().size() is the VALID byte count (may be < allocated).
                let valid_bytes = data.chunk().size() as usize;
                let Some(raw) = data.data() else {
                    return;
                };
                // Clamp to what is actually mapped before slicing.
                let frame_bytes = channels * SAMPLE_STRIDE;
                let usable_bytes = valid_bytes.min(raw.len());
                let frame_count = usable_bytes / frame_bytes;

                let writable_frames = state.producer.slots().min(frame_count);
                if writable_frames > 0 {
                    let chunk = state
                        .producer
                        .write_chunk_uninit(writable_frames)
                        .expect("the producer just reported this many writable slots");
                    let written = chunk.fill_from_iter(
                        raw[..frame_count * frame_bytes]
                            .chunks_exact(frame_bytes)
                            .take(writable_frames)
                            .map(|frame| {
                                // Little-endian F32 (we negotiated F32LE).
                                frame
                                    .as_chunks::<SAMPLE_STRIDE>()
                                    .0
                                    .iter()
                                    .map(|sample| f32::from_le_bytes(*sample))
                                    .sum::<f32>()
                                    / channels as f32
                            }),
                    );
                    debug_assert_eq!(written, writable_frames);
                }

                let dropped = frame_count - writable_frames;
                if dropped > 0 {
                    state.transport.record_overrun(dropped as u64);
                }

                // Publish the boundary write before acknowledging, including
                // when the pause request arrives during the write.
                acknowledge_pause_after_write(&state.transport);
            })
            .register()?;

        // EnumFormat POD: accept F32LE at our pinned rate; leave channels free.
        // (AudioInfoRaw -> Vec<Property> `Into` + the Object/Value/PodSerializer
        // shape match the pipewire-rs upstream audio-capture example.)
        let mut audio_info = AudioInfoRaw::new();
        audio_info.set_format(AudioFormat::F32LE);
        audio_info.set_rate(PIPEWIRE_CAPTURE_RATE);
        let obj = Object {
            type_: SpaTypes::ObjectParamFormat.as_raw(),
            id: ParamType::EnumFormat.as_raw(),
            properties: audio_info.into(),
        };
        let values: Vec<u8> =
            PodSerializer::serialize(Cursor::new(Vec::new()), &Value::Object(obj))
                .expect("failed to serialize EnumFormat POD")
                .0
                .into_inner();
        let mut params = [Pod::from_bytes(&values).expect("serialized POD is malformed")];

        // Connect as INPUT (we consume audio from a source):
        //  - AUTOCONNECT: let the session manager wire us to the source
        //    (default, or our TARGET_OBJECT if set).
        //  - MAP_BUFFERS: CPU-mapped buffers so `data()` returns real memory.
        //  - RT_PROCESS: run `process` on the realtime data thread.
        stream.connect(
            Direction::Input,
            None,
            pw::stream::StreamFlags::AUTOCONNECT
                | pw::stream::StreamFlags::MAP_BUFFERS
                | pw::stream::StreamFlags::RT_PROCESS,
            &mut params,
        )?;

        // Setup succeeded — hand control back to open(), then block processing
        // audio until close() sends a quit. `_listener`, `stream`, `_quit_guard`
        // and `core` stay owned in this scope for the whole run.
        let _ = init_tx.send(Ok(()));
        mainloop.run();
        Ok(())
    })();

    // Only reached-with-Err before init_tx was sent Ok (setup failure). After a
    // successful setup the closure returns Ok(()) once the loop quits.
    if let Err(e) = setup {
        let _ = init_tx.send(Err(format!("PipeWire capture setup failed: {e}")));
    }
}

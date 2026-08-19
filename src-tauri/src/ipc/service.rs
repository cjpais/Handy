//! Transport-independent implementations of the IPC methods. Everything here
//! takes an `AppHandle` and returns plain JSON values or an `RpcError`, so the
//! same logic can later back other transports (CLI verbs, an MCP facade, …)
//! without touching the socket server.

use std::io::Read;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use hound::{SampleFormat, WavReader};
use log::{debug, info};
use serde::Deserialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Manager};

use super::protocol::{ErrorKind, RpcError, PROTOCOL_VERSION};
use crate::audio_toolkit::audio::FrameResampler;
use crate::managers::engine_gate::EngineJobKind;
use crate::managers::model::ModelManager;
use crate::managers::transcription::TranscriptionManager;
use crate::settings::get_settings;

/// Engine sample rate expected by `TranscriptionManager::transcribe`.
const TARGET_SAMPLE_RATE: u32 = 16_000;
/// Reject absurdly long inputs before decoding them into memory.
const MAX_AUDIO_SECS: u64 = 2 * 60 * 60;

/// `initialize` — optional version handshake. Errors only if the client
/// explicitly requests a protocol version this server does not speak.
pub fn initialize(app: &AppHandle, params: Option<&Value>) -> Result<Value, RpcError> {
    #[derive(Deserialize)]
    struct InitParams {
        protocol_version: Option<u64>,
        client: Option<String>,
    }

    let p: InitParams = parse_params(params)?;
    if let Some(requested) = p.protocol_version {
        if requested != PROTOCOL_VERSION {
            return Err(RpcError::new(
                ErrorKind::ProtocolMismatch,
                format!(
                    "client requested protocol version {requested}, server speaks {PROTOCOL_VERSION}"
                ),
            ));
        }
    }
    if let Some(client) = &p.client {
        info!("IPC client connected: {client}");
    }

    Ok(json!({
        "server": "handy",
        "protocol_version": PROTOCOL_VERSION,
        "app_version": app.package_info().version.to_string(),
    }))
}

/// `status` — cheap snapshot of transcription availability. Always answers,
/// even while the model is cold or dictation is running.
pub fn status(app: &AppHandle) -> Value {
    let settings = get_settings(app);
    let selected_model = settings.selected_model;

    let selected_model_installed = app
        .try_state::<Arc<ModelManager>>()
        .and_then(|mm| mm.get_model_info(&selected_model))
        .map(|info| info.is_downloaded)
        .unwrap_or(false);

    // The engine reservation is the single admission authority: a dictation
    // session holds it from recording start (or a background job holds it
    // with the dictation pending behind), so the holder check alone answers
    // "busy" — no recording-state inspection needed.
    let (model_loaded, loaded_model, engine_busy) =
        match app.try_state::<Arc<TranscriptionManager>>() {
            Some(tm) => (
                tm.is_model_loaded(),
                tm.get_current_model(),
                tm.engine_holder().is_some(),
            ),
            None => (false, None, false),
        };

    json!({
        "server": "handy",
        "protocol_version": PROTOCOL_VERSION,
        "app_version": app.package_info().version.to_string(),
        "selected_model": selected_model,
        "selected_model_installed": selected_model_installed,
        "model_loaded": model_loaded,
        "loaded_model": loaded_model,
        "busy": engine_busy,
    })
}

/// `models.list` — the same registry the app and `--list-models --json` use.
pub fn models_list(app: &AppHandle) -> Result<Value, RpcError> {
    let mm = app
        .try_state::<Arc<ModelManager>>()
        .ok_or_else(|| RpcError::new(ErrorKind::Internal, "model manager not initialized"))?;
    let models = serde_json::to_value(mm.get_available_models())
        .map_err(|e| RpcError::new(ErrorKind::Internal, format!("serialize models: {e}")))?;
    Ok(json!({ "models": models }))
}

/// `transcribe.file` — batch-transcribe a WAV file with the user's selected
/// model. Input: absolute path to a PCM WAV (any common rate/bit depth, mono
/// or multi-channel); normalization to 16 kHz mono happens here.
pub fn transcribe_file(app: &AppHandle, params: Option<&Value>) -> Result<Value, RpcError> {
    #[derive(Deserialize)]
    struct TranscribeParams {
        path: String,
    }

    let p: TranscribeParams = parse_params(params)?;
    let path = Path::new(&p.path);
    if !path.is_absolute() {
        return Err(RpcError::new(
            ErrorKind::InvalidParams,
            "path must be absolute",
        ));
    }

    // Cheap setup checks fail fast before any decode work.
    let settings = get_settings(app);
    let model_id = settings.selected_model;
    if model_id.is_empty() {
        return Err(RpcError::new(
            ErrorKind::SetupRequired,
            "no model selected — pick one in the Handy app",
        ));
    }

    let mm = app
        .try_state::<Arc<ModelManager>>()
        .ok_or_else(|| RpcError::new(ErrorKind::Internal, "model manager not initialized"))?;
    let installed = mm
        .get_model_info(&model_id)
        .map(|info| info.is_downloaded)
        .unwrap_or(false);
    if !installed {
        return Err(RpcError::new(
            ErrorKind::ModelNotInstalled,
            format!("model '{model_id}' is not downloaded"),
        ));
    }

    // Decode and normalize BEFORE reserving the engine: a slow disk, a large
    // file, or an expensive resample must not hold the engine admission slot.
    // The reservation protects engine work only.
    let samples = load_wav_16k_mono(path)?;
    let audio_secs = samples.len() as f64 / TARGET_SAMPLE_RATE as f64;

    let tm = app
        .try_state::<Arc<TranscriptionManager>>()
        .ok_or_else(|| {
            RpcError::new(ErrorKind::Internal, "transcription manager not initialized")
        })?;

    // One engine job at a time, app-wide: the reservation is the single
    // admission authority, shared with dictation (which reserves at recording
    // start), history retranscription, and maintenance. Concurrent callers
    // get `busy` immediately rather than queueing. Held through load +
    // transcribe, released before the response is written.
    let reservation = tm
        .try_reserve(EngineJobKind::Ipc)
        .map_err(|busy| RpcError::new(ErrorKind::Busy, busy.to_string()))?;

    // Load (or switch back to) the selected model if needed. This is the same
    // load the app performs on first dictation, so a cold IPC call pays the
    // same cost dictation would.
    let load_start = Instant::now();
    tm.ensure_model_loaded(&reservation, &model_id)
        .map_err(|e| RpcError::new(ErrorKind::ModelLoadFailed, e.to_string()))?;
    let load_ms = load_start.elapsed().as_millis() as u64;

    let transcribe_start = Instant::now();
    let text = tm
        .transcribe(samples, &reservation)
        .map_err(|e| RpcError::new(ErrorKind::Internal, e.to_string()))?;
    let transcribe_ms = transcribe_start.elapsed().as_millis() as u64;

    debug!(
        "IPC transcribe.file: {:.1}s audio in {}ms (load {}ms)",
        audio_secs, transcribe_ms, load_ms
    );

    Ok(json!({
        "text": text,
        "model": model_id,
        "audio_secs": audio_secs,
        "transcribe_ms": transcribe_ms,
        "load_ms": load_ms,
    }))
}

fn parse_params<T: serde::de::DeserializeOwned>(params: Option<&Value>) -> Result<T, RpcError> {
    // Absent params are treated as an empty object, so methods whose params
    // are all optional accept a bare call and required fields still fail
    // with a clear invalid-params error.
    let value = params.cloned().unwrap_or_else(|| json!({}));
    serde_json::from_value(value)
        .map_err(|e| RpcError::new(ErrorKind::InvalidParams, e.to_string()))
}

/// Read a PCM WAV file and normalize it to the engine's 16 kHz mono f32
/// contract: decode (s16/s24/s32/f32), downmix channels by averaging, and
/// resample with the same `FrameResampler` the live mic path uses.
fn load_wav_16k_mono(path: &Path) -> Result<Vec<f32>, RpcError> {
    let reader = WavReader::open(path).map_err(|e| match e {
        hound::Error::IoError(ref io) if io.kind() == std::io::ErrorKind::NotFound => {
            RpcError::new(
                ErrorKind::FileNotFound,
                format!("{}: not found", path.display()),
            )
        }
        other => RpcError::new(
            ErrorKind::UnsupportedAudio,
            format!("{}: {}", path.display(), other),
        ),
    })?;
    decode_wav(reader)
}

fn decode_wav<R: Read>(reader: WavReader<R>) -> Result<Vec<f32>, RpcError> {
    let spec = reader.spec();
    let channels = spec.channels as usize;

    if channels == 0 || channels > 8 {
        return Err(RpcError::new(
            ErrorKind::UnsupportedAudio,
            format!("unsupported channel count: {}", spec.channels),
        ));
    }
    if !(8_000..=192_000).contains(&spec.sample_rate) {
        return Err(RpcError::new(
            ErrorKind::UnsupportedAudio,
            format!("unsupported sample rate: {} Hz", spec.sample_rate),
        ));
    }
    let frames = reader.duration() as u64;
    if frames / spec.sample_rate as u64 > MAX_AUDIO_SECS {
        return Err(RpcError::new(
            ErrorKind::UnsupportedAudio,
            format!("audio longer than {MAX_AUDIO_SECS}s"),
        ));
    }

    let interleaved: Vec<f32> = match (spec.sample_format, spec.bits_per_sample) {
        (SampleFormat::Float, 32) => reader
            .into_samples::<f32>()
            .collect::<Result<_, _>>()
            .map_err(decode_err)?,
        (SampleFormat::Int, 16) => reader
            .into_samples::<i16>()
            .map(|s| s.map(|v| v as f32 / 32_768.0))
            .collect::<Result<_, _>>()
            .map_err(decode_err)?,
        (SampleFormat::Int, 24) => reader
            .into_samples::<i32>()
            .map(|s| s.map(|v| v as f32 / 8_388_608.0))
            .collect::<Result<_, _>>()
            .map_err(decode_err)?,
        (SampleFormat::Int, 32) => reader
            .into_samples::<i32>()
            .map(|s| s.map(|v| v as f32 / 2_147_483_648.0))
            .collect::<Result<_, _>>()
            .map_err(decode_err)?,
        (format, bits) => {
            return Err(RpcError::new(
                ErrorKind::UnsupportedAudio,
                format!("unsupported sample format: {bits}-bit {format:?}"),
            ))
        }
    };

    // Downmix to mono by averaging channels.
    let mono: Vec<f32> = if channels == 1 {
        interleaved
    } else {
        interleaved
            .chunks_exact(channels)
            .map(|frame| frame.iter().sum::<f32>() / channels as f32)
            .collect()
    };

    if spec.sample_rate == TARGET_SAMPLE_RATE {
        return Ok(mono);
    }

    let mut resampler = FrameResampler::new(
        spec.sample_rate as usize,
        TARGET_SAMPLE_RATE as usize,
        Duration::from_millis(30),
    );
    let mut out = Vec::with_capacity(
        (mono.len() as u64 * TARGET_SAMPLE_RATE as u64 / spec.sample_rate as u64) as usize + 480,
    );
    resampler.push(&mono, |frame| out.extend_from_slice(frame));
    resampler.finish(|frame| out.extend_from_slice(frame));
    Ok(out)
}

fn decode_err(e: hound::Error) -> RpcError {
    RpcError::new(ErrorKind::UnsupportedAudio, format!("decode failed: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use hound::{WavSpec, WavWriter};
    use std::io::Cursor;

    fn wav_bytes(
        spec: WavSpec,
        write: impl FnOnce(&mut WavWriter<&mut Cursor<Vec<u8>>>),
    ) -> Vec<u8> {
        let mut cursor = Cursor::new(Vec::new());
        {
            let mut writer = WavWriter::new(&mut cursor, spec).unwrap();
            write(&mut writer);
            writer.finalize().unwrap();
        }
        cursor.into_inner()
    }

    fn decode(bytes: Vec<u8>) -> Result<Vec<f32>, RpcError> {
        decode_wav(WavReader::new(Cursor::new(bytes)).unwrap())
    }

    #[test]
    fn passthrough_16k_mono_i16() {
        let spec = WavSpec {
            channels: 1,
            sample_rate: 16_000,
            bits_per_sample: 16,
            sample_format: SampleFormat::Int,
        };
        let bytes = wav_bytes(spec, |w| {
            for _ in 0..1600 {
                w.write_sample(8192i16).unwrap();
            }
        });
        let samples = decode(bytes).unwrap();
        assert_eq!(samples.len(), 1600);
        assert!((samples[0] - 0.25).abs() < 0.01);
    }

    #[test]
    fn downmixes_and_resamples_48k_stereo_f32() {
        let spec = WavSpec {
            channels: 2,
            sample_rate: 48_000,
            bits_per_sample: 32,
            sample_format: SampleFormat::Float,
        };
        // 0.5 s of DC: left 0.1, right 0.5 → mono 0.3.
        let bytes = wav_bytes(spec, |w| {
            for _ in 0..24_000 {
                w.write_sample(0.1f32).unwrap();
                w.write_sample(0.5f32).unwrap();
            }
        });
        let samples = decode(bytes).unwrap();
        // ~8000 samples at 16 kHz, plus up to one zero-padded 30 ms frame.
        assert!(
            (7_800..=8_600).contains(&samples.len()),
            "unexpected output length {}",
            samples.len()
        );
        // Check the steady-state middle region (skip filter edges).
        let mid = &samples[2_000..6_000];
        let mean = mid.iter().sum::<f32>() / mid.len() as f32;
        assert!((mean - 0.3).abs() < 0.02, "mean {mean} != ~0.3");
    }

    #[test]
    fn rejects_unsupported_bit_depth() {
        let spec = WavSpec {
            channels: 1,
            sample_rate: 16_000,
            bits_per_sample: 8,
            sample_format: SampleFormat::Int,
        };
        let bytes = wav_bytes(spec, |w| {
            for _ in 0..100 {
                w.write_sample(1i8).unwrap();
            }
        });
        let err = decode(bytes).unwrap_err();
        assert_eq!(err.kind, ErrorKind::UnsupportedAudio);
    }
}

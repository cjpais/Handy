//! Wire protocol between Handy and its transcribe.cpp worker process.
//!
//! Strict request/response: every [`Request`] gets exactly one [`Response`].
//! Each message is one frame: `[u32 json_len][json][u32 pcm_len][pcm bytes]`
//! (lengths little-endian, PCM as little-endian f32). JSON (not bincode) is
//! deliberate: transcribe-cpp's serde impls round-trip NaN confidences through
//! `null`, which only works with a self-describing format.

use serde::{Deserialize, Serialize};
use std::io::{self, Read, Write};
use std::path::PathBuf;
use transcribe_cpp::{
    Backend, Capabilities, DeviceType, RunOptions, StreamOptions, StreamText, StreamUpdate,
    Transcript,
};

/// Refuse frames larger than this: a garbage length must not turn into a
/// multi-gigabyte allocation. One hour of 16 kHz f32 PCM is ~230 MB.
const MAX_SECTION_BYTES: usize = 1 << 30;

#[derive(Debug, Serialize, Deserialize)]
pub enum Request {
    /// First request to every worker; answered once backend init is done
    /// (with an error if it registered no compute device). With
    /// `list_devices`, the worker also enumerates its compute devices.
    /// CPU-only workers that hold a model don't list: they would only see
    /// the CPU.
    Hello {
        list_devices: bool,
    },
    Load {
        path: PathBuf,
        backend: Backend,
        device: DeviceSelector,
    },
    /// Followed by the PCM to transcribe.
    Run {
        options: RunOptions,
    },
    StreamBegin {
        run: RunOptions,
        stream: StreamOptions,
    },
    /// Followed by the PCM frame to feed.
    Feed,
    /// `want_language`: also report the model's detected language (costs a
    /// transcript snapshot, so only asked for when it can change the outcome).
    Finalize {
        want_language: bool,
    },
    StreamReset,
}

/// How the worker should pick the device for a [`Request::Load`]. Device
/// handles are process-local, so the parent names devices by stable key or by
/// registry index and the worker resolves them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeviceSelector {
    Auto,
    /// Persisted identity from [`device_key`]. Falls back to automatic
    /// selection when the device is no longer present.
    Key(String),
    /// Registry index from `--list-devices`. Fails if not a usable device.
    Index(usize),
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Response {
    /// `devices` is present only when the hello asked for them.
    Hello {
        devices: Option<Vec<DeviceInfo>>,
    },
    Loaded(LoadedInfo),
    Transcript(Transcript),
    Fed {
        update: StreamUpdate,
        /// Present only when the committed or tentative text changed.
        text: Option<StreamText>,
    },
    Finalized {
        update: StreamUpdate,
        text: StreamText,
        language: Option<String>,
    },
    Ok,
    Error(String),
}

/// Serializable mirror of `transcribe_cpp::Device` (which holds a
/// process-local handle and so has no serde impl).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub name: String,
    pub description: String,
    pub kind: String,
    pub device_type: DeviceType,
    pub device_id: Option<String>,
    pub memory_total: u64,
    pub index: Option<usize>,
    /// Stable identity for persisting a device choice; see [`device_key`].
    pub key: String,
}

impl DeviceInfo {
    pub fn from_device(device: &transcribe_cpp::Device) -> Self {
        Self {
            name: device.name.clone(),
            description: device.description.clone(),
            kind: device.kind.clone(),
            device_type: device.device_type,
            device_id: device.device_id.clone(),
            memory_total: device.memory_total,
            index: device.index,
            key: device_key(&device.kind, device.device_id.as_deref(), &device.name),
        }
    }

    pub fn label(&self) -> &str {
        if self.description.is_empty() {
            &self.name
        } else {
            &self.description
        }
    }

    pub fn is_gpu(&self) -> bool {
        matches!(self.device_type, DeviceType::Gpu | DeviceType::Igpu)
    }
}

/// Persistent device identity. Uses the backend's stable `device_id` where
/// available and its name otherwise (Metal reports no id). Must stay
/// byte-identical to what older versions stored in settings.
pub fn device_key(kind: &str, device_id: Option<&str>, name: &str) -> String {
    let (identity_kind, identity) = match device_id {
        Some(device_id) => ("id", device_id),
        None => ("name", name),
    };
    serde_json::to_string(&(kind, identity_kind, identity))
        .expect("transcribe device identity is always JSON serializable")
}

/// What the parent needs to know about a loaded model, read once at load.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoadedInfo {
    pub arch: String,
    pub variant: String,
    pub backend: String,
    /// Label of the device the model actually bound to.
    pub device: String,
    /// Whether that device is a GPU (a crash there may be the GPU's fault).
    pub on_gpu: bool,
    pub capabilities: Capabilities,
    pub supports_initial_prompt: bool,
}

/// Encode one frame.
pub fn encode_message<T: Serialize>(message: &T, pcm: Option<&[f32]>) -> io::Result<Vec<u8>> {
    let json = serde_json::to_vec(message).map_err(io::Error::other)?;
    let pcm = pcm.unwrap_or(&[]);
    let mut frame = Vec::with_capacity(8 + json.len() + pcm.len() * 4);
    frame.extend_from_slice(&(json.len() as u32).to_le_bytes());
    frame.extend_from_slice(&json);
    frame.extend_from_slice(&((pcm.len() * 4) as u32).to_le_bytes());
    for sample in pcm {
        frame.extend_from_slice(&sample.to_le_bytes());
    }
    Ok(frame)
}

pub fn write_message<T: Serialize>(
    w: &mut impl Write,
    message: &T,
    pcm: Option<&[f32]>,
) -> io::Result<()> {
    w.write_all(&encode_message(message, pcm)?)?;
    w.flush()
}

/// Read one frame. `Ok(None)` only on a clean EOF before the first byte of a
/// frame; a frame cut off part-way is an error.
pub fn read_message<T: for<'de> Deserialize<'de>>(
    r: &mut impl Read,
) -> io::Result<Option<(T, Vec<f32>)>> {
    let mut len = [0u8; 4];
    let mut filled = 0;
    while filled < len.len() {
        match r.read(&mut len[filled..]) {
            Ok(0) if filled == 0 => return Ok(None),
            Ok(0) => return Err(io::ErrorKind::UnexpectedEof.into()),
            Ok(n) => filled += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    let json = read_section_body(r, len)?;
    let message = serde_json::from_slice(&json).map_err(io::Error::other)?;
    let pcm_bytes = read_section(r)?;
    if pcm_bytes.len() % 4 != 0 {
        return Err(io::Error::other(
            "PCM section is not a whole number of f32s",
        ));
    }
    let pcm = pcm_bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| f32::from_le_bytes(*b))
        .collect();
    Ok(Some((message, pcm)))
}

fn read_section(r: &mut impl Read) -> io::Result<Vec<u8>> {
    let mut len = [0u8; 4];
    r.read_exact(&mut len)?;
    read_section_body(r, len)
}

fn read_section_body(r: &mut impl Read, len: [u8; 4]) -> io::Result<Vec<u8>> {
    let len = u32::from_le_bytes(len) as usize;
    if len > MAX_SECTION_BYTES {
        return Err(io::Error::other(format!("frame section too large: {len}")));
    }
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf)?;
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_message_with_pcm() {
        let mut buf = Vec::new();
        let pcm = [0.0f32, -1.0, 0.5, f32::MIN_POSITIVE];
        write_message(
            &mut buf,
            &Request::Run {
                options: RunOptions::default(),
            },
            Some(&pcm),
        )
        .unwrap();
        let (req, got): (Request, Vec<f32>) = read_message(&mut buf.as_slice()).unwrap().unwrap();
        assert!(matches!(req, Request::Run { .. }));
        assert_eq!(got, pcm);
    }

    #[test]
    fn clean_eof_is_none() {
        let empty: &[u8] = &[];
        assert!(read_message::<Request>(&mut &*empty).unwrap().is_none());
    }

    #[test]
    fn truncated_frame_is_an_error() {
        let mut buf = Vec::new();
        write_message(
            &mut buf,
            &Request::Run {
                options: RunOptions::default(),
            },
            Some(&[0.5f32; 8]),
        )
        .unwrap();
        // Cut inside the length prefix, the JSON, the PCM length and the PCM.
        for cut in [1, 3, 6, buf.len() - 34, buf.len() - 1] {
            let partial = &buf[..cut];
            let err = read_message::<Request>(&mut &*partial).unwrap_err();
            assert_eq!(err.kind(), io::ErrorKind::UnexpectedEof, "cut at {cut}");
        }
    }

    #[test]
    fn device_key_matches_legacy_format() {
        assert_eq!(
            device_key("metal", None, "Metal"),
            r#"["metal","name","Metal"]"#
        );
        assert_eq!(
            device_key("vulkan", Some("0000:01:00.0"), "Vulkan0"),
            r#"["vulkan","id","0000:01:00.0"]"#
        );
    }
}

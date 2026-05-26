//! Per-engine accelerator preferences.
//!
//! Each engine family has its own accelerator enum containing only the options
//! meaningful for that engine. Call the appropriate setter early in your program
//! before loading models.

use std::fmt;
use std::str::FromStr;
use std::sync::atomic::{AtomicU8, Ordering};

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// ORT accelerator
// ---------------------------------------------------------------------------

/// Preferred hardware accelerator for ORT-based engines (SenseVoice, GigaAM, Parakeet, Moonshine).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[repr(u8)]
pub enum OrtAccelerator {
    /// Automatically select the best available execution provider (default).
    Auto = 0,
    /// Force CPU-only execution — no GPU providers.
    CpuOnly = 1,
    /// NVIDIA CUDA.
    Cuda = 2,
    /// Microsoft DirectML (Windows).
    DirectMl = 3,
    /// AMD ROCm.
    Rocm = 4,
}

static ORT_ACCELERATOR: AtomicU8 = AtomicU8::new(OrtAccelerator::Auto as u8);

/// Set the global ORT accelerator preference.
///
/// Call once, early in the program, before any ORT models are loaded.
pub fn set_ort_accelerator(pref: OrtAccelerator) {
    ORT_ACCELERATOR.store(pref as u8, Ordering::Relaxed);
}

/// Get the current ORT accelerator preference.
pub fn get_ort_accelerator() -> OrtAccelerator {
    OrtAccelerator::from_u8(ORT_ACCELERATOR.load(Ordering::Relaxed))
}

impl OrtAccelerator {
    /// Return the list of ORT accelerators that are compiled-in for the current build.
    ///
    /// Always includes `CpuOnly`. Only includes GPU accelerators whose corresponding
    /// feature flag is enabled.
    pub fn available() -> Vec<OrtAccelerator> {
        #[allow(unused_mut)]
        let mut v = vec![OrtAccelerator::CpuOnly];

        #[cfg(feature = "ort-cuda")]
        v.push(OrtAccelerator::Cuda);

        #[cfg(feature = "ort-directml")]
        v.push(OrtAccelerator::DirectMl);

        #[cfg(feature = "ort-rocm")]
        v.push(OrtAccelerator::Rocm);

        v
    }

    fn from_u8(val: u8) -> Self {
        match val {
            0 => Self::Auto,
            1 => Self::CpuOnly,
            2 => Self::Cuda,
            3 => Self::DirectMl,
            4 => Self::Rocm,
            _ => Self::Auto,
        }
    }
}

impl Default for OrtAccelerator {
    fn default() -> Self {
        Self::Auto
    }
}

impl fmt::Display for OrtAccelerator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::Auto => "auto",
            Self::CpuOnly => "cpu",
            Self::Cuda => "cuda",
            Self::DirectMl => "directml",
            Self::Rocm => "rocm",
        };
        f.write_str(s)
    }
}

impl FromStr for OrtAccelerator {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "auto" => Ok(Self::Auto),
            "cpu" | "cpu_only" | "cpuonly" => Ok(Self::CpuOnly),
            "cuda" => Ok(Self::Cuda),
            "directml" | "dml" => Ok(Self::DirectMl),
            "rocm" => Ok(Self::Rocm),
            other => Err(format!("unknown ORT accelerator: {other}")),
        }
    }
}

// ---------------------------------------------------------------------------
// Whisper accelerator
// ---------------------------------------------------------------------------

/// Preferred hardware accelerator for the whisper.cpp engine.
///
/// The actual GPU backend (Metal, Vulkan, etc.) is selected at compile time
/// via whisper-rs feature flags. This enum only controls whether GPU is used.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[repr(u8)]
pub enum WhisperAccelerator {
    /// Automatically select the best available backend (default — uses GPU if available).
    Auto = 0,
    /// Force CPU-only execution.
    CpuOnly = 1,
    /// Explicitly request GPU execution.
    Gpu = 2,
}

static WHISPER_ACCELERATOR: AtomicU8 = AtomicU8::new(WhisperAccelerator::Auto as u8);

/// Set the global whisper.cpp accelerator preference.
///
/// Call once, early in the program, before any Whisper models are loaded.
pub fn set_whisper_accelerator(pref: WhisperAccelerator) {
    WHISPER_ACCELERATOR.store(pref as u8, Ordering::Relaxed);
}

/// Get the current whisper.cpp accelerator preference.
pub fn get_whisper_accelerator() -> WhisperAccelerator {
    WhisperAccelerator::from_u8(WHISPER_ACCELERATOR.load(Ordering::Relaxed))
}

impl WhisperAccelerator {
    /// Returns `true` if GPU should be used.
    pub fn use_gpu(&self) -> bool {
        *self != Self::CpuOnly
    }

    fn from_u8(val: u8) -> Self {
        match val {
            0 => Self::Auto,
            1 => Self::CpuOnly,
            2 => Self::Gpu,
            _ => Self::Auto,
        }
    }
}

impl Default for WhisperAccelerator {
    fn default() -> Self {
        Self::Auto
    }
}

impl fmt::Display for WhisperAccelerator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::Auto => "auto",
            Self::CpuOnly => "cpu",
            Self::Gpu => "gpu",
        };
        f.write_str(s)
    }
}

impl FromStr for WhisperAccelerator {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "auto" => Ok(Self::Auto),
            "cpu" | "cpu_only" | "cpuonly" => Ok(Self::CpuOnly),
            "gpu" => Ok(Self::Gpu),
            other => Err(format!("unknown Whisper accelerator: {other}")),
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// RAII guard that restores Auto preference for both engines when dropped.
    struct AccelGuard;
    impl Drop for AccelGuard {
        fn drop(&mut self) {
            set_ort_accelerator(OrtAccelerator::Auto);
            set_whisper_accelerator(WhisperAccelerator::Auto);
        }
    }

    // -- ORT tests --

    #[test]
    fn ort_default_is_auto() {
        let _g = AccelGuard;
        set_ort_accelerator(OrtAccelerator::Auto);
        assert_eq!(get_ort_accelerator(), OrtAccelerator::Auto);
    }

    #[test]
    fn ort_set_and_get() {
        let _g = AccelGuard;
        set_ort_accelerator(OrtAccelerator::Cuda);
        assert_eq!(get_ort_accelerator(), OrtAccelerator::Cuda);
        set_ort_accelerator(OrtAccelerator::CpuOnly);
        assert_eq!(get_ort_accelerator(), OrtAccelerator::CpuOnly);
    }

    #[test]
    fn ort_display_roundtrip() {
        for pref in [
            OrtAccelerator::Auto,
            OrtAccelerator::CpuOnly,
            OrtAccelerator::Cuda,
            OrtAccelerator::DirectMl,
            OrtAccelerator::Rocm,
        ] {
            let s = pref.to_string();
            let parsed: OrtAccelerator = s.parse().unwrap();
            assert_eq!(parsed, pref);
        }
    }

    #[test]
    fn ort_parse_aliases() {
        assert_eq!(
            "dml".parse::<OrtAccelerator>().unwrap(),
            OrtAccelerator::DirectMl
        );
        assert_eq!(
            "CPU".parse::<OrtAccelerator>().unwrap(),
            OrtAccelerator::CpuOnly
        );
        assert_eq!(
            "cpu_only".parse::<OrtAccelerator>().unwrap(),
            OrtAccelerator::CpuOnly
        );
    }

    #[test]
    fn ort_parse_unknown_errors() {
        assert!("foobar".parse::<OrtAccelerator>().is_err());
    }

    #[test]
    fn ort_serde_roundtrip() {
        let pref = OrtAccelerator::Cuda;
        let json = serde_json::to_string(&pref).unwrap();
        assert_eq!(json, "\"cuda\"");
        let back: OrtAccelerator = serde_json::from_str(&json).unwrap();
        assert_eq!(back, pref);
    }

    #[test]
    fn ort_available_always_includes_cpu() {
        let avail = OrtAccelerator::available();
        assert!(avail.contains(&OrtAccelerator::CpuOnly));
    }

    #[test]
    fn ort_from_u8_invalid_returns_auto() {
        assert_eq!(OrtAccelerator::from_u8(255), OrtAccelerator::Auto);
    }

    // -- Whisper tests --

    #[test]
    fn whisper_default_is_auto() {
        let _g = AccelGuard;
        set_whisper_accelerator(WhisperAccelerator::Auto);
        assert_eq!(get_whisper_accelerator(), WhisperAccelerator::Auto);
    }

    #[test]
    fn whisper_set_and_get() {
        let _g = AccelGuard;
        set_whisper_accelerator(WhisperAccelerator::CpuOnly);
        assert_eq!(get_whisper_accelerator(), WhisperAccelerator::CpuOnly);
        set_whisper_accelerator(WhisperAccelerator::Gpu);
        assert_eq!(get_whisper_accelerator(), WhisperAccelerator::Gpu);
    }

    #[test]
    fn whisper_display_roundtrip() {
        for pref in [
            WhisperAccelerator::Auto,
            WhisperAccelerator::CpuOnly,
            WhisperAccelerator::Gpu,
        ] {
            let s = pref.to_string();
            let parsed: WhisperAccelerator = s.parse().unwrap();
            assert_eq!(parsed, pref);
        }
    }

    #[test]
    fn whisper_use_gpu_flag() {
        assert!(WhisperAccelerator::Auto.use_gpu());
        assert!(!WhisperAccelerator::CpuOnly.use_gpu());
        assert!(WhisperAccelerator::Gpu.use_gpu());
    }

    #[test]
    fn whisper_parse_unknown_errors() {
        assert!("foobar".parse::<WhisperAccelerator>().is_err());
    }

    #[test]
    fn whisper_serde_roundtrip() {
        let pref = WhisperAccelerator::Gpu;
        let json = serde_json::to_string(&pref).unwrap();
        assert_eq!(json, "\"gpu\"");
        let back: WhisperAccelerator = serde_json::from_str(&json).unwrap();
        assert_eq!(back, pref);
    }

    #[test]
    fn whisper_from_u8_invalid_returns_auto() {
        assert_eq!(WhisperAccelerator::from_u8(255), WhisperAccelerator::Auto);
    }
}

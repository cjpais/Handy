use ort::execution_providers::CPUExecutionProvider;
#[cfg(feature = "ort-cuda")]
use ort::execution_providers::CUDAExecutionProvider;
#[cfg(feature = "ort-directml")]
use ort::execution_providers::DirectMLExecutionProvider;
#[cfg(feature = "ort-rocm")]
use ort::execution_providers::ROCmExecutionProvider;

use ort::session::builder::GraphOptimizationLevel;
use ort::session::Session;
use std::path::Path;

use crate::accel::{get_ort_accelerator, OrtAccelerator};

/// Build the execution provider list based on the global accelerator preference.
fn execution_providers() -> Vec<ort::execution_providers::ExecutionProviderDispatch> {
    let pref = get_ort_accelerator();
    let mut eps = Vec::new();

    match pref {
        OrtAccelerator::CpuOnly => {
            // CPU only — no GPU providers
        }
        OrtAccelerator::Cuda => {
            #[cfg(feature = "ort-cuda")]
            eps.push(CUDAExecutionProvider::default().build());
            #[cfg(not(feature = "ort-cuda"))]
            log::warn!(
                "Accelerator set to CUDA but ort-cuda feature is not enabled; falling back to CPU"
            );
        }
        OrtAccelerator::DirectMl => {
            #[cfg(feature = "ort-directml")]
            eps.push(DirectMLExecutionProvider::default().build());
            #[cfg(not(feature = "ort-directml"))]
            log::warn!("Accelerator set to DirectML but ort-directml feature is not enabled; falling back to CPU");
        }
        OrtAccelerator::Rocm => {
            #[cfg(feature = "ort-rocm")]
            eps.push(ROCmExecutionProvider::default().build());
            #[cfg(not(feature = "ort-rocm"))]
            log::warn!(
                "Accelerator set to ROCm but ort-rocm feature is not enabled; falling back to CPU"
            );
        }
        OrtAccelerator::Auto => {
            // Add compiled-in GPU EPs in priority order.
            // DirectML is excluded from Auto because it requires
            // parallel_execution(false) and memory_pattern(false),
            // which would penalize other backends. Use
            // OrtAccelerator::DirectMl explicitly for DirectML.
            #[cfg(feature = "ort-cuda")]
            eps.push(CUDAExecutionProvider::default().build());
            #[cfg(feature = "ort-rocm")]
            eps.push(ROCmExecutionProvider::default().build());
        }
    }

    // CPU is always the final fallback
    eps.push(CPUExecutionProvider::default().build());
    eps
}

/// Returns true if DirectML is the explicitly selected execution provider.
fn directml_active() -> bool {
    get_ort_accelerator() == OrtAccelerator::DirectMl && cfg!(feature = "ort-directml")
}

/// Internal session builder with full control over threading and EP selection.
fn build_session(
    path: &Path,
    intra_threads: Option<usize>,
    parallel_execution: bool,
) -> Result<Session, ort::Error> {
    let mut builder =
        Session::builder()?.with_optimization_level(GraphOptimizationLevel::Level3)?;

    if let Some(n) = intra_threads {
        if n > 0 {
            builder = builder.with_intra_threads(n)?;
        }
    }

    // DirectML requires parallel_execution(false) and memory_pattern(false)
    let use_parallel = if directml_active() {
        false
    } else {
        parallel_execution
    };

    builder = builder.with_parallel_execution(use_parallel)?;

    if directml_active() {
        builder = builder.with_memory_pattern(false)?;
    }

    let session = builder
        .with_execution_providers(execution_providers())?
        .commit_from_file(path)?;

    for input in &session.inputs {
        log::info!(
            "Model input: name={}, type={:?}",
            input.name,
            input.input_type
        );
    }
    for output in &session.outputs {
        log::info!(
            "Model output: name={}, type={:?}",
            output.name,
            output.output_type
        );
    }

    Ok(session)
}

/// Create an ONNX session with standard settings.
pub fn create_session(path: &Path) -> Result<Session, ort::Error> {
    build_session(path, None, true)
}

/// Create an ONNX session with configurable thread count.
pub fn create_session_with_threads(path: &Path, num_threads: usize) -> Result<Session, ort::Error> {
    build_session(path, Some(num_threads), true)
}

/// Resolve a model file path for the requested quantization level.
///
/// Looks for `{name}.{suffix}.onnx` based on the quantization variant,
/// falling back to `{name}.onnx` (FP32) if the requested file doesn't exist.
pub fn resolve_model_path(
    dir: &Path,
    name: &str,
    quantization: &super::Quantization,
) -> std::path::PathBuf {
    let suffix = match quantization {
        super::Quantization::FP32 => None,
        super::Quantization::FP16 => Some("fp16"),
        super::Quantization::Int8 => Some("int8"),
    };

    if let Some(suffix) = suffix {
        let path = dir.join(format!("{}.{}.onnx", name, suffix));
        if path.exists() {
            log::info!("Loading {} model: {}", suffix, path.display());
            return path;
        }
        log::warn!(
            "{} model not found at {}, falling back to {}.onnx",
            suffix,
            path.display(),
            name
        );
    }

    dir.join(format!("{}.onnx", name))
}

/// Read a custom metadata string from an ONNX session.
pub fn read_metadata_str(session: &Session, key: &str) -> Result<Option<String>, ort::Error> {
    let meta = session.metadata()?;
    meta.custom(key)
}

/// Read a custom metadata i32 value, with optional default.
pub fn read_metadata_i32(
    session: &Session,
    key: &str,
    default: Option<i32>,
) -> Result<Option<i32>, crate::TranscribeError> {
    let str_val = read_metadata_str(session, key).map_err(|e| {
        crate::TranscribeError::Config(format!("failed to read metadata '{}': {}", key, e))
    })?;
    match str_val {
        Some(v) => Ok(Some(v.parse::<i32>().map_err(|e| {
            crate::TranscribeError::Config(format!("failed to parse '{}': {}", key, e))
        })?)),
        None => Ok(default),
    }
}

/// Read a comma-separated float vector from metadata.
pub fn read_metadata_float_vec(
    session: &Session,
    key: &str,
) -> Result<Option<Vec<f32>>, crate::TranscribeError> {
    let str_val = read_metadata_str(session, key).map_err(|e| {
        crate::TranscribeError::Config(format!("failed to read metadata '{}': {}", key, e))
    })?;
    match str_val {
        Some(v) => {
            let floats: Result<Vec<f32>, _> =
                v.split(',').map(|s| s.trim().parse::<f32>()).collect();
            Ok(Some(floats.map_err(|e| {
                crate::TranscribeError::Config(format!(
                    "failed to parse floats in '{}': {}",
                    key, e
                ))
            })?))
        }
        None => Ok(None),
    }
}

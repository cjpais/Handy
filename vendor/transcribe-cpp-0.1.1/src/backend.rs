//! Backend module loading and compute-device discovery.
//!
//! In a static (or plain `shared`) build the compiled-in backends are already
//! registered and [`init_backends`] is a harmless no-op. In a `dynamic-backends`
//! build the compute backends are loadable modules; a host points the library at
//! the provider directory ONCE, before the first model load — use
//! [`init_backends_default`] when the modules sit next to libtranscribe, or
//! [`init_backends`] with an explicit bundled provider directory. See the C
//! header's backend-module section for the degradation contract.

use std::ffi::CString;
use std::path::Path;

use transcribe_cpp_sys as sys;

use crate::error::{check, Result};
use crate::result::{owned_opt_str, owned_str};
use crate::types::Backend;

/// ggml's vendor-agnostic class for a compute device, orthogonal to
/// [`Device::kind`] (which carries the vendor). Backends report this
/// classification themselves, so use it as a runtime hint rather than a
/// portable hardware-memory taxonomy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceType {
    /// CPU using system memory.
    Cpu,
    /// Backend-reported GPU.
    Gpu,
    /// Backend-reported integrated GPU.
    Igpu,
    /// Host-memory accelerator (BLAS/AMX/...).
    Accel,
    /// A device-type value this binding does not recognize — reported by a
    /// runtime newer than the header this binding was generated against.
    /// Distinguish such devices by [`Device::device_id`] / [`Device::name`],
    /// not by this axis.
    Unknown,
}

impl DeviceType {
    fn from_raw(raw: sys::transcribe_device_type) -> Self {
        use sys::transcribe_device_type as T;
        match raw {
            T::TRANSCRIBE_DEVICE_TYPE_CPU => DeviceType::Cpu,
            T::TRANSCRIBE_DEVICE_TYPE_GPU => DeviceType::Gpu,
            T::TRANSCRIBE_DEVICE_TYPE_IGPU => DeviceType::Igpu,
            T::TRANSCRIBE_DEVICE_TYPE_ACCEL => DeviceType::Accel,
            _ => DeviceType::Unknown,
        }
    }
}

/// One registered compute device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    /// ggml device name, e.g. "Metal".
    pub name: String,
    /// Human-readable description, e.g. "Apple M4 Max".
    pub description: String,
    /// Classified vendor kind: "cpu", "accel", "metal", "vulkan", "cuda",
    /// "sycl", "gpu", or "unknown".
    pub kind: String,
    /// The CPU/GPU/IGPU/ACCEL axis, orthogonal to [`Device::kind`].
    pub device_type: DeviceType,
    /// Stable hardware id when the backend reports one (PCI bus id for PCI
    /// devices), or `None` (e.g. Metal).
    pub device_id: Option<String>,
    /// Reported device memory capacity in bytes, or 0 if unreported.
    pub memory_total: u64,
    /// Available device memory in bytes — a snapshot at the time this was
    /// queried, or 0 if unreported. Re-query (via [`devices`] or
    /// [`crate::Model::device`]) to refresh it; the value is backend-defined
    /// and not comparable across device kinds.
    pub memory_free: u64,
    /// Registry index of this device — the value to pass as
    /// [`ModelOptions::gpu_device`](crate::ModelOptions) to select it (0
    /// selects the auto / first device). `None` when this `Device` came from
    /// [`crate::Model::device`], since `transcribe_model_get_device` does not
    /// expose an index; correlate such a device back to [`devices`] by
    /// `device_id` / `name` instead. Order-dependent and not stable across
    /// driver updates or hosts.
    pub index: Option<usize>,
}

impl Device {
    /// Build a [`Device`] from the raw FFI struct filled by the library.
    /// `index` is the registry index when the device came from enumeration,
    /// or `None` when it came from `transcribe_model_get_device`.
    pub(crate) fn from_raw(raw: &sys::transcribe_backend_device, index: Option<usize>) -> Device {
        Device {
            name: owned_str(raw.name),
            description: owned_str(raw.description),
            kind: owned_str(raw.kind),
            device_type: DeviceType::from_raw(raw.device_type),
            device_id: owned_opt_str(raw.device_id),
            memory_total: raw.memory_total,
            memory_free: raw.memory_free,
            index,
        }
    }
}

/// Load backend modules from `dir` (dynamic builds) and register their
/// devices. A no-op in static builds. Call once, before the first model load.
///
/// Errors with [`crate::Error::Backend`] if, after loading, the process has no
/// registered compute device (a dynamic build pointed at a directory with no
/// usable modules). This call is idempotent per directory and NOT retryable in
/// the same process.
pub fn init_backends(dir: impl AsRef<Path>) -> Result<()> {
    let dir = dir.as_ref();
    // Pass the path bytes through faithfully (Unix) / reject non-UTF-8 (Windows),
    // matching model loading — never lossily mangle a path with to_string_lossy.
    let c_dir = CString::new(crate::model::path_bytes(dir)?)?;
    let status = unsafe { sys::transcribe_init_backends(c_dir.as_ptr()) };
    check(status, "init_backends")
}

/// Load the backend modules the way the build expects, with no caller-supplied
/// path.
///
/// - **Compiled-in builds** (the default static build, or a plain `shared`
///   build): a no-op returning `Ok(())` — the backends are already registered.
/// - **`dynamic-backends` builds**: resolves the directory containing the
///   loaded libtranscribe and scans only that directory. Ship the backend
///   modules next to libtranscribe for this helper to find them.
///
/// If your app uses a different layout, call [`init_backends`] with that
/// resolved module directory instead. Like [`init_backends`], this is
/// idempotent and must run once before the first model load.
pub fn init_backends_default() -> Result<()> {
    let status = unsafe { sys::transcribe_init_backends_default() };
    check(status, "init_backends_default")
}

/// The number of compute devices currently registered.
pub fn device_count() -> usize {
    let n = unsafe { sys::transcribe_backend_device_count() };
    n.max(0) as usize
}

/// Every registered compute device.
pub fn devices() -> Vec<Device> {
    let mut out = Vec::with_capacity(device_count());
    for i in 0..device_count() as i32 {
        let mut raw: sys::transcribe_backend_device = unsafe { std::mem::zeroed() };
        unsafe { sys::transcribe_backend_device_init(&mut raw) };
        let status = unsafe { sys::transcribe_get_backend_device(i, &mut raw) };
        if status == sys::transcribe_status::TRANSCRIBE_OK {
            out.push(Device::from_raw(&raw, Some(i as usize)));
        }
    }
    out
}

/// Whether a backend request can be satisfied by some registered device. This
/// is the probe to turn `Backend::Vulkan` on a machine without Vulkan into a
/// clear error instead of a failed model load.
pub fn backend_available(backend: Backend) -> bool {
    unsafe { sys::transcribe_backend_available(backend.to_raw()) }
}

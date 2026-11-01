//! Worker engine for Intel NPUs: Whisper through OpenVINO GenAI's
//! `WhisperPipeline`, which transcribe.cpp's ggml backends can't reach.
//!
//! A worker started with [`OPENVINO_FLAG`] serves the same protocol as the
//! transcribe.cpp worker, but runs this engine. OpenVINO GenAI is loaded at
//! runtime through its C API, so it is optional: without it (or without an
//! NPU) the device is simply not listed. The parent finds it on the library
//! path, or under `INTEL_OPENVINO_DIR` as set by OpenVINO's `setupvars`.
//!
//! The model is the OpenVINO IR export of the Whisper model Handy selected,
//! e.g. `OpenVINO/whisper-large-v3-turbo-int8-ov` for
//! `whisper-large-v3-turbo-Q8_0.gguf`, found in the Hugging Face cache that
//! holds the GGUF, or under `HANDY_OPENVINO_MODELS`.
//!
//! [`OPENVINO_FLAG`]: super::OPENVINO_FLAG

use super::protocol::{write_message, DeviceInfo, DeviceSelector, LoadedInfo, Request, Response};
use super::worker::Incoming;
use libloading::Library;
use log::{debug, error, info};
use std::ffi::{c_char, c_int, CStr, CString};
use std::fs::File;
use std::path::{Path, PathBuf};
use std::ptr;
use std::sync::mpsc::Receiver;
use std::time::Instant;
use transcribe_cpp::{Capabilities, DeviceType, RunOptions, Task, TimestampKind, Transcript};

/// `DeviceInfo::kind` of the devices this engine lists.
pub const KIND: &str = "openvino";
/// OpenVINO devices offered for transcription. CPU and GPU are already
/// served by transcribe.cpp.
const DEVICES: &[&str] = &["NPU"];
/// Whisper decodes 30 s windows; longer audio is chunked by the parent.
const MAX_AUDIO_MS: i64 = 30_000;

/// Serve requests with this engine until the parent closes stdin.
pub(super) fn serve(mut output: File, requests: &Receiver<Incoming>) -> i32 {
    let api = Api::load();
    let mut loaded: Option<(Pipeline, LoadedInfo)> = None;
    while let Ok(Incoming { request, pcm }) = requests.recv() {
        let response = match (&api, request) {
            (Err(e), _) => Response::Error(e.clone()),
            (Ok(api), Request::Hello { list_devices }) => Response::Hello {
                devices: list_devices.then(|| api.devices()),
            },
            (Ok(api), Request::Load { path, device, .. }) => {
                loaded = None;
                match load(api, &path, &device) {
                    Ok(model) => {
                        let info = model.1.clone();
                        loaded = Some(model);
                        Response::Loaded(info)
                    }
                    Err(e) => Response::Error(e),
                }
            }
            (Ok(_), Request::Run { options }) => match &loaded {
                Some((pipeline, info)) => match pipeline.run(&pcm, &options, info) {
                    Ok(transcript) => Response::Transcript(transcript),
                    Err(e) => Response::Error(e),
                },
                None => Response::Error("no model loaded".to_string()),
            },
            (Ok(_), _) => Response::Error("OpenVINO models do not stream".to_string()),
        };
        if let Err(e) = write_message(&mut output, &response, None) {
            error!("Protocol write failed: {}", e);
            return 1;
        }
    }
    0
}

/// Library directories of the OpenVINO install named by `INTEL_OPENVINO_DIR`,
/// for the parent to put on the worker's library path. Empty when unset.
pub fn library_dirs() -> Vec<PathBuf> {
    let Some(root) = std::env::var_os("INTEL_OPENVINO_DIR").map(PathBuf::from) else {
        return Vec::new();
    };
    let runtime = root.join("runtime");
    #[cfg(windows)]
    let dirs = [
        runtime.join("bin").join("intel64").join("Release"),
        runtime.join("3rdparty").join("tbb").join("bin"),
    ];
    #[cfg(not(windows))]
    let dirs = [
        runtime.join("lib").join("intel64"),
        runtime.join("3rdparty").join("tbb").join("lib"),
    ];
    dirs.into_iter().filter(|d| d.is_dir()).collect()
}

/// Whether a load should go to this engine: it names an OpenVINO device.
pub fn selects(selector: &DeviceSelector) -> bool {
    matches!(
        selector,
        DeviceSelector::Key(key) if DEVICES.iter().any(|d| *key == device_info(d).key)
    )
}

fn device_info(device: &str) -> DeviceInfo {
    let device_id = format!("{KIND}:{device}");
    DeviceInfo {
        name: format!("OPENVINO-{device}"),
        description: format!("Intel {device} (OpenVINO)"),
        kind: KIND.to_string(),
        // Listed as a GPU so it can be picked like one.
        device_type: DeviceType::Gpu,
        key: super::protocol::device_key(KIND, Some(&device_id), device),
        device_id: Some(device_id),
        memory_total: 0,
        index: None,
    }
}

fn load<'a>(
    api: &'a Api,
    gguf: &Path,
    selector: &DeviceSelector,
) -> Result<(Pipeline<'a>, LoadedInfo), String> {
    let device = DEVICES
        .iter()
        .find(|d| matches!(selector, DeviceSelector::Key(key) if *key == device_info(d).key))
        .ok_or_else(|| format!("{selector:?} is not an OpenVINO device"))?;
    let dir = model_dir(gguf)?;
    let config = ModelConfig::read(&dir)?;
    let started = Instant::now();
    let pipeline = api.pipeline(&dir, device)?;
    info!(
        "Loaded {} on {} in {}ms",
        dir.display(),
        device,
        started.elapsed().as_millis()
    );
    let info = LoadedInfo {
        arch: "whisper".to_string(),
        variant: dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
        backend: "OpenVINO".to_string(),
        device: device_info(device).description,
        on_gpu: true,
        capabilities: Capabilities {
            native_sample_rate: 16_000,
            translate_target_languages: if config.translate {
                vec!["en".to_string()]
            } else {
                Vec::new()
            },
            languages: config.languages,
            max_timestamp_kind: TimestampKind::None,
            supports_language_detect: config.multilingual,
            supports_translate: config.translate,
            supports_streaming: false,
            supports_spec_decode: false,
            max_audio_ms: MAX_AUDIO_MS,
        },
        supports_initial_prompt: true,
        supports_long_form: false,
        supports_vocabulary: true,
    };
    Ok((pipeline, info))
}

/// The OpenVINO IR export matching a Whisper GGUF:
/// `<name>[-<quant>].gguf` -> `<name>-int8-ov` (or `<name>-ov`).
fn model_dir(gguf: &Path) -> Result<PathBuf, String> {
    let stem = gguf
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| format!("bad model path {}", gguf.display()))?;
    let name = match stem.rsplit_once('-') {
        Some((name, quant)) if is_quant(quant) => name,
        _ => stem,
    };
    let names = [format!("{name}-int8-ov"), format!("{name}-ov")];
    let is_model = |dir: &Path| dir.join("openvino_encoder_model.xml").is_file();

    if let Some(root) = std::env::var_os("HANDY_OPENVINO_MODELS").map(PathBuf::from) {
        if let Some(dir) = names.iter().map(|n| root.join(n)).find(|d| is_model(d)) {
            return Ok(dir);
        }
    }
    // A Hugging Face cache path: <hub>/models--<org>--<repo>/snapshots/<rev>/<file>.
    if let Some(hub) = gguf.ancestors().nth(4) {
        for name in &names {
            let snapshots = hub
                .join(format!("models--OpenVINO--{name}"))
                .join("snapshots");
            let Ok(entries) = std::fs::read_dir(&snapshots) else {
                continue;
            };
            if let Some(dir) = entries.flatten().map(|e| e.path()).find(|d| is_model(d)) {
                return Ok(dir);
            }
        }
    }
    Err(format!(
        "No OpenVINO export of {name} found: download OpenVINO/{} from Hugging Face",
        names[0]
    ))
}

fn is_quant(s: &str) -> bool {
    let s = s.to_ascii_uppercase();
    s.starts_with('Q') || s.starts_with("IQ") || ["F16", "F32", "BF16"].contains(&s.as_str())
}

/// What `generation_config.json` says about the model.
struct ModelConfig {
    languages: Vec<String>,
    multilingual: bool,
    translate: bool,
}

impl ModelConfig {
    fn read(dir: &Path) -> Result<Self, String> {
        let path = dir.join("generation_config.json");
        let json: serde_json::Value = std::fs::read(&path)
            .map_err(|e| e.to_string())
            .and_then(|bytes| serde_json::from_slice(&bytes).map_err(|e| e.to_string()))
            .map_err(|e| format!("Failed to read {}: {e}", path.display()))?;
        let languages: Vec<String> = json["lang_to_id"]
            .as_object()
            .map(|langs| {
                langs
                    .keys()
                    .map(|k| {
                        k.trim_start_matches("<|")
                            .trim_end_matches("|>")
                            .to_string()
                    })
                    .collect()
            })
            .unwrap_or_default();
        // Turbo keeps the translate token but was not trained for it
        // (transcribe.cpp's catalog marks it untranslatable too).
        let turbo = dir.to_string_lossy().contains("turbo");
        Ok(Self {
            multilingual: json["is_multilingual"]
                .as_bool()
                .unwrap_or(!languages.is_empty()),
            translate: !turbo && json["task_to_id"].get("translate").is_some(),
            languages,
        })
    }
}

/// Where compiled NPU blobs are cached, so loads after the first skip the
/// (minute-long) compile.
fn cache_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
    #[cfg(not(windows))]
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")));
    let dir = base?.join("com.pais.handy").join("openvino");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

// ---- OpenVINO (GenAI) C API, loaded at runtime ----

#[repr(C)]
struct OvAvailableDevices {
    devices: *mut *mut c_char,
    size: usize,
}

type Status = c_int;
type Opaque = std::ffi::c_void;

/// The C API functions this engine calls. The libraries stay loaded for the
/// life of the process.
struct Api {
    _libs: Vec<Library>,
    last_err_msg: unsafe extern "C" fn() -> *const c_char,
    core_create: unsafe extern "C" fn(*mut *mut Opaque) -> Status,
    core_free: unsafe extern "C" fn(*mut Opaque),
    core_get_available_devices:
        unsafe extern "C" fn(*const Opaque, *mut OvAvailableDevices) -> Status,
    available_devices_free: unsafe extern "C" fn(*mut OvAvailableDevices),
    pipeline_create:
        unsafe extern "C" fn(*const c_char, *const c_char, usize, *mut *mut Opaque, ...) -> Status,
    pipeline_free: unsafe extern "C" fn(*mut Opaque),
    pipeline_generate: unsafe extern "C" fn(
        *mut Opaque,
        *const f32,
        usize,
        *const Opaque,
        *mut *mut Opaque,
    ) -> Status,
    pipeline_get_generation_config: unsafe extern "C" fn(*const Opaque, *mut *mut Opaque) -> Status,
    config_free: unsafe extern "C" fn(*mut Opaque),
    config_set_language: unsafe extern "C" fn(*mut Opaque, *const c_char) -> Status,
    config_set_task: unsafe extern "C" fn(*mut Opaque, *const c_char) -> Status,
    config_set_initial_prompt: unsafe extern "C" fn(*mut Opaque, *const c_char) -> Status,
    config_set_return_timestamps: unsafe extern "C" fn(*mut Opaque, bool) -> Status,
    results_free: unsafe extern "C" fn(*mut Opaque),
    results_get_string: unsafe extern "C" fn(*const Opaque, *mut c_char, *mut usize) -> Status,
}

#[cfg(windows)]
const LIBS: [&str; 2] = ["openvino_c.dll", "openvino_genai_c.dll"];
#[cfg(not(windows))]
const LIBS: [&str; 2] = ["libopenvino_c.so", "libopenvino_genai_c.so"];

impl Api {
    fn load() -> Result<Self, String> {
        let libs = LIBS
            .iter()
            .map(|name| open_library(name))
            .collect::<Result<Vec<_>, _>>()?;
        let (c, genai) = (&libs[0], &libs[1]);
        macro_rules! sym {
            ($lib:expr, $name:literal) => {
                // SAFETY: the declared type matches the C API's prototype.
                unsafe {
                    *$lib
                        .get(concat!($name, "\0").as_bytes())
                        .map_err(|e| format!("OpenVINO symbol {}: {e}", $name))?
                }
            };
        }
        Ok(Self {
            last_err_msg: sym!(c, "ov_get_last_err_msg"),
            core_create: sym!(c, "ov_core_create"),
            core_free: sym!(c, "ov_core_free"),
            core_get_available_devices: sym!(c, "ov_core_get_available_devices"),
            available_devices_free: sym!(c, "ov_available_devices_free"),
            pipeline_create: sym!(genai, "ov_genai_whisper_pipeline_create"),
            pipeline_free: sym!(genai, "ov_genai_whisper_pipeline_free"),
            pipeline_generate: sym!(genai, "ov_genai_whisper_pipeline_generate"),
            pipeline_get_generation_config: sym!(
                genai,
                "ov_genai_whisper_pipeline_get_generation_config"
            ),
            config_free: sym!(genai, "ov_genai_whisper_generation_config_free"),
            config_set_language: sym!(genai, "ov_genai_whisper_generation_config_set_language"),
            config_set_task: sym!(genai, "ov_genai_whisper_generation_config_set_task"),
            config_set_initial_prompt: sym!(
                genai,
                "ov_genai_whisper_generation_config_set_initial_prompt"
            ),
            config_set_return_timestamps: sym!(
                genai,
                "ov_genai_whisper_generation_config_set_return_timestamps"
            ),
            results_free: sym!(genai, "ov_genai_whisper_decoded_results_free"),
            results_get_string: sym!(genai, "ov_genai_whisper_decoded_results_get_string"),
            _libs: libs,
        })
    }

    fn check(&self, status: Status, what: &str) -> Result<(), String> {
        if status == 0 {
            return Ok(());
        }
        // SAFETY: returns a NUL-terminated thread-local message, or null.
        let message = unsafe { (self.last_err_msg)() };
        let message = if message.is_null() {
            String::new()
        } else {
            // SAFETY: non-null, NUL-terminated, valid until the next C API call.
            unsafe { CStr::from_ptr(message) }
                .to_string_lossy()
                .trim()
                .to_string()
        };
        Err(format!("OpenVINO {what} failed ({status}): {message}"))
    }

    /// The OpenVINO devices this engine offers that are present.
    fn devices(&self) -> Vec<DeviceInfo> {
        let mut core = ptr::null_mut();
        // SAFETY: out-pointers are valid; everything created here is freed here.
        let available = unsafe {
            if let Err(e) = self.check((self.core_create)(&mut core), "core init") {
                error!("{e}");
                return Vec::new();
            }
            let mut list = OvAvailableDevices {
                devices: ptr::null_mut(),
                size: 0,
            };
            let names = match self.check(
                (self.core_get_available_devices)(core, &mut list),
                "device listing",
            ) {
                Ok(()) => (0..list.size)
                    .map(|i| {
                        CStr::from_ptr(*list.devices.add(i))
                            .to_string_lossy()
                            .into_owned()
                    })
                    .collect::<Vec<_>>(),
                Err(e) => {
                    error!("{e}");
                    Vec::new()
                }
            };
            if !list.devices.is_null() {
                (self.available_devices_free)(&mut list);
            }
            (self.core_free)(core);
            names
        };
        debug!("OpenVINO devices: {:?}", available);
        DEVICES
            .iter()
            .filter(|d| available.iter().any(|a| a == *d))
            .map(|d| device_info(d))
            .collect()
    }

    fn pipeline(&self, dir: &Path, device: &str) -> Result<Pipeline<'_>, String> {
        let path = c_string(&dir.to_string_lossy())?;
        let device = c_string(device)?;
        let cache = cache_dir()
            .map(|d| c_string(&d.to_string_lossy()))
            .transpose()?;
        let mut handle = ptr::null_mut();
        // SAFETY: valid C strings; property args come in key/value pairs, two per pair.
        let status = unsafe {
            match &cache {
                Some(cache) => (self.pipeline_create)(
                    path.as_ptr(),
                    device.as_ptr(),
                    2,
                    &mut handle,
                    c"CACHE_DIR".as_ptr(),
                    cache.as_ptr(),
                ),
                None => (self.pipeline_create)(path.as_ptr(), device.as_ptr(), 0, &mut handle),
            }
        };
        self.check(status, "Whisper pipeline load")?;
        Ok(Pipeline { api: self, handle })
    }
}

fn open_library(name: &str) -> Result<Library, String> {
    // The parent put INTEL_OPENVINO_DIR's libraries on the library path, so
    // the libraries' own dependencies resolve too.
    // SAFETY: OpenVINO's libraries have no unsafe load-time initialisers.
    unsafe { Library::new(name) }.map_err(|e| format!("OpenVINO is not available: {e}"))
}

fn c_string(s: &str) -> Result<CString, String> {
    CString::new(s).map_err(|_| format!("NUL byte in {s:?}"))
}

/// A loaded `WhisperPipeline`.
struct Pipeline<'a> {
    api: &'a Api,
    handle: *mut Opaque,
}

impl Pipeline<'_> {
    fn run(
        &self,
        pcm: &[f32],
        options: &RunOptions,
        info: &LoadedInfo,
    ) -> Result<Transcript, String> {
        let api = self.api;
        let language = options
            .language
            .as_deref()
            .filter(|l| info.capabilities.languages.iter().any(|known| known == l))
            .map(|l| c_string(&format!("<|{l}|>")))
            .transpose()?;
        let task = c_string(match options.task {
            Task::Translate => "translate",
            _ => "transcribe",
        })?;
        let prompt = options
            .prompt
            .clone()
            .or_else(|| (!options.vocabulary.is_empty()).then(|| options.vocabulary.join(", ")))
            .map(|p| c_string(&p))
            .transpose()?;

        let started = Instant::now();
        let mut config = ptr::null_mut();
        let mut results = ptr::null_mut();
        // SAFETY: handles come from the C API and are freed exactly once below;
        // strings outlive the calls that borrow them.
        let text = unsafe {
            api.check(
                (api.pipeline_get_generation_config)(self.handle, &mut config),
                "config",
            )?;
            let generated = (|| -> Result<String, String> {
                let opt = |s: &Option<CString>| s.as_ref().map_or(ptr::null(), |s| s.as_ptr());
                api.check(
                    (api.config_set_language)(config, opt(&language)),
                    "set language",
                )?;
                api.check((api.config_set_task)(config, task.as_ptr()), "set task")?;
                api.check(
                    (api.config_set_initial_prompt)(config, opt(&prompt)),
                    "set prompt",
                )?;
                api.check(
                    (api.config_set_return_timestamps)(config, false),
                    "set timestamps",
                )?;
                api.check(
                    (api.pipeline_generate)(
                        self.handle,
                        pcm.as_ptr(),
                        pcm.len(),
                        config,
                        &mut results,
                    ),
                    "transcription",
                )?;
                let mut size = 0;
                api.check(
                    (api.results_get_string)(results, ptr::null_mut(), &mut size),
                    "result",
                )?;
                let mut buf = vec![0u8; size.max(1)];
                api.check(
                    (api.results_get_string)(results, buf.as_mut_ptr().cast(), &mut size),
                    "result",
                )?;
                Ok(CStr::from_bytes_until_nul(&buf)
                    .map(|s| s.to_string_lossy().trim().to_string())
                    .unwrap_or_default())
            })();
            if !results.is_null() {
                (api.results_free)(results);
            }
            (api.config_free)(config);
            generated?
        };
        debug!(
            "OpenVINO run: {:.1}s of audio in {}ms",
            pcm.len() as f64 / 16_000.0,
            started.elapsed().as_millis()
        );
        Ok(Transcript {
            raw_text: text.clone(),
            text,
            // The C API does not report the detected language.
            language: options.language.clone(),
            ..Default::default()
        })
    }
}

impl Drop for Pipeline<'_> {
    fn drop(&mut self) {
        // SAFETY: created by pipeline_create and freed only here.
        unsafe { (self.api.pipeline_free)(self.handle) }
    }
}

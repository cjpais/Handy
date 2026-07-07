//! Idiomatic Rust enums mirroring the C API's parameter enums.
//!
//! Each maps to/from the generated `transcribe-cpp-sys` NewType. Conversions
//! INTO C are total; conversions OUT of C validate the raw integer and fall
//! back to a safe default for an unknown value (enum hygiene — we never assume
//! the library only ever hands back values this version knows about).

use transcribe_cpp_sys as sys;

/// The task a run performs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Task {
    /// Transcribe speech in its source language.
    #[default]
    Transcribe,
    /// Translate speech into the target language (model must support it).
    Translate,
}

impl Task {
    pub(crate) fn to_raw(self) -> sys::transcribe_task {
        match self {
            Task::Transcribe => sys::transcribe_task::TRANSCRIBE_TASK_TRANSCRIBE,
            Task::Translate => sys::transcribe_task::TRANSCRIBE_TASK_TRANSLATE,
        }
    }
}

/// Requested (or returned) timestamp granularity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TimestampKind {
    /// Text only, no alignment data.
    None,
    /// "Richest the model supports" — resolved per-family at run time.
    #[default]
    Auto,
    /// Segment-level start/end times.
    Segment,
    /// Word-level start/end times.
    Word,
    /// Token-level start/end times.
    Token,
}

impl TimestampKind {
    pub(crate) fn to_raw(self) -> sys::transcribe_timestamp_kind {
        use sys::transcribe_timestamp_kind as K;
        match self {
            TimestampKind::None => K::TRANSCRIBE_TIMESTAMPS_NONE,
            TimestampKind::Auto => K::TRANSCRIBE_TIMESTAMPS_AUTO,
            TimestampKind::Segment => K::TRANSCRIBE_TIMESTAMPS_SEGMENT,
            TimestampKind::Word => K::TRANSCRIBE_TIMESTAMPS_WORD,
            TimestampKind::Token => K::TRANSCRIBE_TIMESTAMPS_TOKEN,
        }
    }

    pub(crate) fn from_raw(raw: sys::transcribe_timestamp_kind) -> Self {
        use sys::transcribe_timestamp_kind as K;
        match raw {
            K::TRANSCRIBE_TIMESTAMPS_AUTO => TimestampKind::Auto,
            K::TRANSCRIBE_TIMESTAMPS_SEGMENT => TimestampKind::Segment,
            K::TRANSCRIBE_TIMESTAMPS_WORD => TimestampKind::Word,
            K::TRANSCRIBE_TIMESTAMPS_TOKEN => TimestampKind::Token,
            // includes TRANSCRIBE_TIMESTAMPS_NONE and any unknown value
            _ => TimestampKind::None,
        }
    }
}

/// K/V activation precision for the decoder's flash-attention path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum KvType {
    /// f16 for quantized weights, f32 for f32 weights. The best default.
    #[default]
    Auto,
    /// Full-precision K/V.
    F32,
    /// Half-precision K/V.
    F16,
}

impl KvType {
    pub(crate) fn to_raw(self) -> sys::transcribe_kv_type {
        use sys::transcribe_kv_type as K;
        match self {
            KvType::Auto => K::TRANSCRIBE_KV_TYPE_AUTO,
            KvType::F32 => K::TRANSCRIBE_KV_TYPE_F32,
            KvType::F16 => K::TRANSCRIBE_KV_TYPE_F16,
        }
    }
}

/// Punctuation + capitalization runtime toggle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Pnc {
    /// The family's shipped default (what its published WER was measured at).
    #[default]
    Default,
    /// Explicitly disable runtime PNC (supporting families only).
    Off,
    /// Explicitly enable runtime PNC (supporting families only).
    On,
}

impl Pnc {
    pub(crate) fn to_raw(self) -> sys::transcribe_pnc_mode {
        use sys::transcribe_pnc_mode as P;
        match self {
            Pnc::Default => P::TRANSCRIBE_PNC_MODE_DEFAULT,
            Pnc::Off => P::TRANSCRIBE_PNC_MODE_OFF,
            Pnc::On => P::TRANSCRIBE_PNC_MODE_ON,
        }
    }
}

/// Inverse-text-normalization runtime toggle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Itn {
    /// The family's shipped default.
    #[default]
    Default,
    /// Explicitly disable runtime ITN (supporting families only).
    Off,
    /// Explicitly enable runtime ITN (supporting families only).
    On,
}

impl Itn {
    pub(crate) fn to_raw(self) -> sys::transcribe_itn_mode {
        use sys::transcribe_itn_mode as I;
        match self {
            Itn::Default => I::TRANSCRIBE_ITN_MODE_DEFAULT,
            Itn::Off => I::TRANSCRIBE_ITN_MODE_OFF,
            Itn::On => I::TRANSCRIBE_ITN_MODE_ON,
        }
    }
}

/// Which compute backend to request when loading a model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Backend {
    /// Best available device; CPU is the always-present fallback. The default.
    #[default]
    Auto,
    /// Strict CPU (no GPU, no host accelerators) — the determinism choice.
    Cpu,
    /// CPU plus host-memory accelerators (BLAS/AMX) when present.
    CpuAccel,
    /// Require Metal; errors if this build has no Metal.
    Metal,
    /// Require Vulkan; errors if this build has no Vulkan.
    Vulkan,
    /// Require CUDA; errors if this build has no CUDA.
    Cuda,
}

impl Backend {
    pub(crate) fn to_raw(self) -> sys::transcribe_backend_request {
        use sys::transcribe_backend_request as B;
        match self {
            Backend::Auto => B::TRANSCRIBE_BACKEND_AUTO,
            Backend::Cpu => B::TRANSCRIBE_BACKEND_CPU,
            Backend::CpuAccel => B::TRANSCRIBE_BACKEND_CPU_ACCEL,
            Backend::Metal => B::TRANSCRIBE_BACKEND_METAL,
            Backend::Vulkan => B::TRANSCRIBE_BACKEND_VULKAN,
            Backend::Cuda => B::TRANSCRIBE_BACKEND_CUDA,
        }
    }
}

/// A yes/no model capability probe (`transcribe_model_supports`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Feature {
    /// Accepts a free-text/token decode prompt (whisper today).
    InitialPrompt,
    /// Runs a multi-tier temperature fallback loop (whisper today).
    TemperatureFallback,
    /// Handles audio longer than its native window in one run (whisper today).
    LongForm,
    /// Honors the abort callback between chunks/decode steps.
    Cancellation,
    /// Exposes a runtime PNC toggle.
    Pnc,
    /// Exposes a runtime ITN toggle.
    Itn,
}

impl Feature {
    pub(crate) fn to_raw(self) -> sys::transcribe_feature {
        use sys::transcribe_feature as F;
        match self {
            Feature::InitialPrompt => F::TRANSCRIBE_FEATURE_INITIAL_PROMPT,
            Feature::TemperatureFallback => F::TRANSCRIBE_FEATURE_TEMPERATURE_FALLBACK,
            Feature::LongForm => F::TRANSCRIBE_FEATURE_LONG_FORM,
            Feature::Cancellation => F::TRANSCRIBE_FEATURE_CANCELLATION,
            Feature::Pnc => F::TRANSCRIBE_FEATURE_PNC,
            Feature::Itn => F::TRANSCRIBE_FEATURE_ITN,
        }
    }
}

/// When the UI-facing committed text grows during a stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CommitPolicy {
    /// The family's stable-prefix implementation. The default.
    #[default]
    Auto,
    /// Commit nothing during feed; finalize commits the final text.
    OnFinalize,
    /// Commit a raw prefix accepted by the stable-prefix implementation.
    StablePrefix,
}

impl CommitPolicy {
    pub(crate) fn to_raw(self) -> sys::transcribe_stream_commit_policy {
        use sys::transcribe_stream_commit_policy as C;
        match self {
            CommitPolicy::Auto => C::TRANSCRIBE_STREAM_COMMIT_AUTO,
            CommitPolicy::OnFinalize => C::TRANSCRIBE_STREAM_COMMIT_ON_FINALIZE,
            CommitPolicy::StablePrefix => C::TRANSCRIBE_STREAM_COMMIT_STABLE_PREFIX,
        }
    }
}

/// Stream lifecycle state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamState {
    Idle,
    Active,
    Finished,
    Failed,
}

impl StreamState {
    pub(crate) fn from_raw(raw: sys::transcribe_stream_state) -> Self {
        use sys::transcribe_stream_state as S;
        match raw {
            S::TRANSCRIBE_STREAM_ACTIVE => StreamState::Active,
            S::TRANSCRIBE_STREAM_FINISHED => StreamState::Finished,
            S::TRANSCRIBE_STREAM_FAILED => StreamState::Failed,
            _ => StreamState::Idle,
        }
    }
}

/// A public ABI struct, for size/alignment introspection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AbiStruct {
    ModelLoadParams,
    SessionParams,
    RunParams,
    StreamParams,
    Capabilities,
    Timings,
    Segment,
    Word,
    Token,
    StreamUpdate,
    StreamText,
    SessionLimits,
    Ext,
    BackendDevice,
}

impl AbiStruct {
    pub(crate) fn to_raw(self) -> sys::transcribe_abi_struct {
        use sys::transcribe_abi_struct as A;
        match self {
            AbiStruct::ModelLoadParams => A::TRANSCRIBE_ABI_MODEL_LOAD_PARAMS,
            AbiStruct::SessionParams => A::TRANSCRIBE_ABI_SESSION_PARAMS,
            AbiStruct::RunParams => A::TRANSCRIBE_ABI_RUN_PARAMS,
            AbiStruct::StreamParams => A::TRANSCRIBE_ABI_STREAM_PARAMS,
            AbiStruct::Capabilities => A::TRANSCRIBE_ABI_CAPABILITIES,
            AbiStruct::Timings => A::TRANSCRIBE_ABI_TIMINGS,
            AbiStruct::Segment => A::TRANSCRIBE_ABI_SEGMENT,
            AbiStruct::Word => A::TRANSCRIBE_ABI_WORD,
            AbiStruct::Token => A::TRANSCRIBE_ABI_TOKEN,
            AbiStruct::StreamUpdate => A::TRANSCRIBE_ABI_STREAM_UPDATE,
            AbiStruct::StreamText => A::TRANSCRIBE_ABI_STREAM_TEXT,
            AbiStruct::SessionLimits => A::TRANSCRIBE_ABI_SESSION_LIMITS,
            AbiStruct::Ext => A::TRANSCRIBE_ABI_EXT,
            AbiStruct::BackendDevice => A::TRANSCRIBE_ABI_BACKEND_DEVICE,
        }
    }
}

/// The slot a family extension is pointed at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtSlot {
    /// `transcribe_run_params::family`.
    Run,
    /// `transcribe_stream_params::family`.
    Stream,
}

impl ExtSlot {
    pub(crate) fn to_raw(self) -> sys::transcribe_ext_slot {
        use sys::transcribe_ext_slot as E;
        match self {
            ExtSlot::Run => E::TRANSCRIBE_EXT_SLOT_RUN,
            ExtSlot::Stream => E::TRANSCRIBE_EXT_SLOT_STREAM,
        }
    }
}

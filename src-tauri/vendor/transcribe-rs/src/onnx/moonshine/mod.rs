mod model;
mod streaming;

pub use model::{MoonshineModel, MoonshineParams};
pub use streaming::{MoonshineStreamingParams, StreamingConfig, StreamingModel, StreamingState};

pub const SAMPLE_RATE: u32 = 16000;

/// Moonshine model variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoonshineVariant {
    Tiny,
    TinyAr,
    TinyZh,
    TinyJa,
    TinyKo,
    TinyUk,
    TinyVi,
    Base,
    BaseEs,
}

impl MoonshineVariant {
    pub fn num_layers(&self) -> usize {
        match self {
            MoonshineVariant::Tiny
            | MoonshineVariant::TinyAr
            | MoonshineVariant::TinyZh
            | MoonshineVariant::TinyJa
            | MoonshineVariant::TinyKo
            | MoonshineVariant::TinyUk
            | MoonshineVariant::TinyVi => 6,
            MoonshineVariant::Base | MoonshineVariant::BaseEs => 8,
        }
    }

    pub fn num_key_value_heads(&self) -> usize {
        8
    }

    pub fn head_dim(&self) -> usize {
        match self {
            MoonshineVariant::Tiny
            | MoonshineVariant::TinyAr
            | MoonshineVariant::TinyZh
            | MoonshineVariant::TinyJa
            | MoonshineVariant::TinyKo
            | MoonshineVariant::TinyUk
            | MoonshineVariant::TinyVi => 36,
            MoonshineVariant::Base | MoonshineVariant::BaseEs => 52,
        }
    }

    pub fn token_rate(&self) -> usize {
        match self {
            MoonshineVariant::Tiny | MoonshineVariant::Base | MoonshineVariant::BaseEs => 6,
            MoonshineVariant::TinyUk => 8,
            MoonshineVariant::TinyAr
            | MoonshineVariant::TinyZh
            | MoonshineVariant::TinyJa
            | MoonshineVariant::TinyKo
            | MoonshineVariant::TinyVi => 13,
        }
    }
}

impl Default for MoonshineVariant {
    fn default() -> Self {
        MoonshineVariant::Tiny
    }
}

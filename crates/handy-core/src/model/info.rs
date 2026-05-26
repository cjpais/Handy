use super::EngineType;
use serde::{Deserialize, Serialize};

/// User-facing model description plus runtime download state.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub filename: String,
    pub url: Option<String>,
    pub size_mb: u64,
    pub is_downloaded: bool,
    pub is_downloading: bool,
    pub partial_size: u64,
    pub is_directory: bool,
    pub engine_type: EngineType,
    /// 0.0 - 1.0, higher is more accurate.
    pub accuracy_score: f32,
    /// 0.0 - 1.0, higher is faster.
    pub speed_score: f32,
    /// Whether the model supports translating to English.
    pub supports_translation: bool,
    /// Whether this is the recommended model for new users.
    pub is_recommended: bool,
    /// Languages this model can transcribe.
    pub supported_languages: Vec<String>,
    /// Whether the user can explicitly pick a language.
    pub supports_language_selection: bool,
    /// Whether this is a user-provided custom model.
    pub is_custom: bool,
}

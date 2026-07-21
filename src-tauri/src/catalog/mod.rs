//! The bundled, offline model catalog (plus the frozen legacy list).
//!
//! `catalog.json` is generated at build time by `scripts/gen_catalog.py` from the
//! `handy-computer` Hugging Face org (card `transcribe_cpp` capabilities +
//! benchmarks, a GGUF header probe for name/params, and local curation for the
//! recommended set). It is compiled into the binary so Handy ships a complete
//! model list with zero network access.
//!
//! `legacy.json` is the retired hardcoded table (Url-hosted blob.handy.computer
//! models), frozen at extraction time and deliberately *not* produced by the
//! generator: it has no upstream to regenerate from, and its ids/filenames are
//! persisted in user settings and on user disks, so it only ever shrinks.
//!
//! Each entry is normalised into a [`ModelDescriptor`] — the same source-agnostic
//! shape every other producer (HF discovery, on-disk scans, the legacy table)
//! yields — so the catalog is "just another producer". Its explicit `capabilities`
//! map becomes a [`CapabilityProbe`] with confident `Some(..)` values; the runtime
//! `GgufHeaderProber` is the same shape with `None` where a header omits a key,
//! which is why the two are interchangeable (the catalog is a baked probe).

use std::collections::{HashMap, HashSet};

use once_cell::sync::Lazy;
use serde::Deserialize;

use crate::managers::model::{
    default_quant_file, EngineType, ModelDescriptor, ModelSource, QuantFile,
};
use crate::managers::model_capabilities::{CapabilityProbe, Compatibility};

#[derive(Deserialize)]
struct CatalogRoot {
    models: Vec<CatalogModel>,
}

/// One model as written in `catalog.json`. Only the fields the descriptor needs
/// are declared; serde ignores the rest (slug, family, license, …).
#[derive(Deserialize)]
struct CatalogModel {
    /// HF repo id, e.g. `handy-computer/whisper-small-gguf`.
    id: String,
    name: String,
    description: String,
    architecture: Option<String>,
    languages: Vec<String>,
    capabilities: CatalogCaps,
    speed_score: Option<f32>,
    accuracy_score: Option<f32>,
    files: Vec<QuantFile>,
    default_quant: Option<String>,
    recommended_rank: Option<u32>,
    /// Part of the small curated onboarding set (badged "Recommended"). Distinct
    /// from `recommended_rank`, which only orders the full list.
    #[serde(default)]
    recommended: bool,
}

#[derive(Deserialize)]
struct CatalogCaps {
    streaming: bool,
    translate: bool,
    lang_detect: bool,
    // `timestamps` (a string enum) is present in the catalog but has no
    // `CapabilityProbe` field yet — wire it through when the probe gains one.
}

impl From<CatalogModel> for ModelDescriptor {
    fn from(m: CatalogModel) -> Self {
        // The default download file. Its name is folded into the id so a catalog
        // entry collides (dedups) with the very same file later discovered in
        // the HF cache — both compute `"{repo_id}/{filename}"`.
        let default_filename = default_quant_file(&m.files, m.default_quant.as_deref())
            .map(|f| f.filename.clone())
            .unwrap_or_default();

        ModelDescriptor {
            id: format!("{}/{}", m.id, default_filename),
            source: ModelSource::HuggingFace {
                repo_id: m.id,
                revision: "main".to_string(),
            },
            name: m.name,
            description: m.description,
            engine_type: EngineType::TranscribeCpp,
            caps: CapabilityProbe {
                verdict: Compatibility::Compatible, // curated org models we ship support for
                display_name: None,
                architecture: m.architecture,
                variant: None,
                languages: Some(m.languages),
                supports_streaming: Some(m.capabilities.streaming),
                supports_translation: Some(m.capabilities.translate),
                supports_language_detect: Some(m.capabilities.lang_detect),
            },
            files: m.files,
            default_quant: m.default_quant,
            // catalog scores are 0–100; ModelInfo / the UI bars use 0.0–1.0.
            speed_score: m.speed_score.unwrap_or(0.0) / 100.0,
            accuracy_score: m.accuracy_score.unwrap_or(0.0) / 100.0,
            recommended_rank: m.recommended_rank,
            recommended: m.recommended,
            is_directory: false,
            deprecated: false,
            supports_language_selection: None,
        }
    }
}

/// One model as written in `legacy.json` — the frozen spec of the retired
/// hardcoded table (Url-hosted downloads from blob.handy.computer). The file
/// is generated-once, hand-owned, and never regenerated: these models have no
/// upstream source of truth, and exist only so users who already downloaded
/// them keep working. Unlike catalog.json, scores here are already normalised
/// 0.0–1.0 (verbatim from the retired table).
#[derive(Deserialize)]
struct LegacyModel {
    id: String,
    name: String,
    description: String,
    filename: String,
    source: ModelSource,
    size_mb: u64,
    is_directory: bool,
    engine_type: EngineType,
    accuracy_score: f32,
    speed_score: f32,
    supports_translation: bool,
    is_recommended: bool,
    languages: Vec<String>,
    /// Explicit, not derived from language count: some legacy engines are
    /// multilingual but take no language parameter (ONNX Parakeet V3).
    supports_language_selection: bool,
    supports_streaming: bool,
    supports_language_detection: bool,
}

impl From<LegacyModel> for ModelDescriptor {
    fn from(m: LegacyModel) -> Self {
        ModelDescriptor {
            id: m.id,
            source: m.source,
            name: m.name,
            description: m.description,
            engine_type: m.engine_type,
            caps: CapabilityProbe {
                verdict: Compatibility::Compatible,
                display_name: None,
                architecture: None,
                variant: None,
                languages: Some(m.languages),
                supports_streaming: Some(m.supports_streaming),
                supports_translation: Some(m.supports_translation),
                supports_language_detect: Some(m.supports_language_detection),
            },
            files: vec![QuantFile {
                filename: m.filename,
                quant: String::new(),
                // Exact inverse of to_model_info's `size_bytes / (1024*1024)`,
                // so the rendered size_mb round-trips the retired table's value.
                size_bytes: m.size_mb * 1024 * 1024,
            }],
            default_quant: None,
            speed_score: m.speed_score,
            accuracy_score: m.accuracy_score,
            recommended_rank: None,
            recommended: m.is_recommended,
            is_directory: m.is_directory,
            deprecated: true,
            supports_language_selection: Some(m.supports_language_selection),
        }
    }
}

/// The bundled catalog, parsed once and normalised into descriptors.
pub static CATALOG: Lazy<Vec<ModelDescriptor>> = Lazy::new(|| {
    let root: CatalogRoot = serde_json::from_str(include_str!("catalog.json"))
        .expect("bundled catalog.json is valid JSON matching the catalog schema");
    root.models.into_iter().map(ModelDescriptor::from).collect()
});

#[derive(Deserialize)]
struct LegacyRoot {
    models: Vec<LegacyModel>,
}

/// The frozen legacy models, parsed once and normalised into the same
/// descriptor shape as the catalog.
pub static LEGACY: Lazy<Vec<ModelDescriptor>> = Lazy::new(|| {
    let root: LegacyRoot = serde_json::from_str(include_str!("legacy.json"))
        .expect("bundled legacy.json is valid JSON matching the legacy schema");
    root.models.into_iter().map(ModelDescriptor::from).collect()
});

/// Every descriptor id in the bundled catalog.
static CATALOG_IDS: Lazy<HashSet<String>> =
    Lazy::new(|| CATALOG.iter().map(|d| d.id.clone()).collect());

/// Whether a registry id was seeded from the bundled catalog. Policies that
/// only apply to entries Handy itself ships — e.g. the models-dir drop-in
/// override — use this to avoid affecting entries discovered from the shared
/// HF cache, which may collide on filename with a catalog model.
pub fn is_catalog_model(model_id: &str) -> bool {
    CATALOG_IDS.contains(model_id)
}

/// Editorial recommended rank keyed by descriptor id (the same id the model
/// registry uses). Built once from the catalog.
static RANK_BY_ID: Lazy<HashMap<String, u32>> = Lazy::new(|| {
    CATALOG
        .iter()
        .filter_map(|d| d.recommended_rank.map(|r| (d.id.clone(), r)))
        .collect()
});

/// Recommended rank for a model id (lower = higher priority). Returns
/// `u32::MAX` for unranked/unknown ids so they sort last in an ascending sort.
pub fn rank_of(model_id: &str) -> u32 {
    RANK_BY_ID.get(model_id).copied().unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::managers::model_capabilities::KNOWN_ARCHES;
    use std::collections::BTreeSet;

    #[test]
    fn catalog_parses_and_is_nonempty() {
        assert!(!CATALOG.is_empty(), "bundled catalog should contain models");
    }

    #[test]
    fn ids_are_unique_across_catalog_and_legacy() {
        let mut ids: Vec<&str> = CATALOG
            .iter()
            .chain(LEGACY.iter())
            .map(|d| d.id.as_str())
            .collect();
        ids.sort_unstable();
        let before = ids.len();
        ids.dedup();
        assert_eq!(before, ids.len(), "descriptor ids must be unique");
    }

    #[test]
    fn legacy_parses_and_is_frozen_shape() {
        assert_eq!(LEGACY.len(), 16, "legacy.json is frozen; it only shrinks");
        for d in LEGACY.iter() {
            assert!(d.deprecated, "{} must be marked deprecated", d.id);
            assert!(
                matches!(d.source, ModelSource::Url { .. }),
                "{} legacy models are Url-hosted",
                d.id
            );
            assert!(
                !is_catalog_model(&d.id),
                "{} must not count as a catalog model (drop-in override scope)",
                d.id
            );
        }
    }

    #[test]
    fn scores_are_normalised_0_to_1() {
        for d in CATALOG.iter() {
            assert!((0.0..=1.0).contains(&d.speed_score), "{} speed", d.id);
            assert!((0.0..=1.0).contains(&d.accuracy_score), "{} acc", d.id);
        }
    }

    #[test]
    fn is_catalog_model_matches_catalog_ids() {
        for d in CATALOG.iter() {
            assert!(
                is_catalog_model(&d.id),
                "{} should be a catalog model",
                d.id
            );
        }
        assert!(!is_catalog_model("someorg/some-repo/some-file.gguf"));
        assert!(!is_catalog_model("small"));
    }

    #[test]
    fn catalog_architectures_are_known_to_capability_probe() {
        let missing: BTreeSet<&str> = CATALOG
            .iter()
            .filter_map(|d| d.caps.architecture.as_deref())
            .filter(|arch| !KNOWN_ARCHES.contains(arch))
            .collect();

        assert!(
            missing.is_empty(),
            "catalog architecture(s) missing from KNOWN_ARCHES: {:?}",
            missing
        );
    }
}

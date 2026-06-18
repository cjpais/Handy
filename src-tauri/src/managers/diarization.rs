use anyhow::Result;
use log::{debug, info};
use polyvoice::clusterer::KMeansClusterer; // Or AHC Clusterer if available
use polyvoice::embedder::ResNet34Adapter;
use polyvoice::models::ModelRegistry;
use polyvoice::pipeline_v2::hybrid::HybridPipeline;
use polyvoice::segmentation::{PowersetConfig, PowersetSegmenter};
use polyvoice::types::{Profile, SampleRate};
use std::sync::Arc;
use tauri::async_runtime::Mutex;

pub struct DiarizationManager {
    pipeline: Arc<Mutex<Option<HybridPipeline>>>,
    models_dir: std::path::PathBuf,
}

impl DiarizationManager {
    pub fn new(models_dir: std::path::PathBuf) -> Self {
        Self {
            pipeline: Arc::new(Mutex::new(None)),
            models_dir,
        }
    }

    pub async fn init(&self) -> Result<()> {
        let mut pipeline_lock = self.pipeline.lock().await;
        if pipeline_lock.is_some() {
            return Ok(());
        }

        info!("Initializing DiarizationManager (polyvoice)");
        let registry = ModelRegistry::with_cache_dir(&self.models_dir)?;
        let models = registry.ensure_for_profile(Profile::Balanced)?;

        let segmenter = PowersetSegmenter::with_config(
            &models.segmenter_path,
            PowersetConfig {
                window_secs: 10.0,
                hop_secs: 1.0,
                sample_rate: 16000,
                aggregation: Default::default(),
            },
        )?;

        let pool_size = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4);

        let embedder = ResNet34Adapter::new(&models.embedder_path, pool_size)?;

        // Use 20 max speakers for KMeans as a starting point.
        let clusterer = KMeansClusterer::new(20);

        let pipeline =
            HybridPipeline::new(Box::new(segmenter), Box::new(embedder), Box::new(clusterer));

        *pipeline_lock = Some(pipeline);
        info!("DiarizationManager initialized successfully");

        Ok(())
    }

    pub async fn diarize(
        &self,
        audio_samples: &[f32],
        sample_rate_hz: u32,
    ) -> Result<polyvoice::types::DiarizationResult> {
        let pipeline_lock = self.pipeline.lock().await;
        let pipeline = pipeline_lock
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("DiarizationManager not initialized"))?;

        let sample_rate = SampleRate::new(sample_rate_hz)
            .ok_or_else(|| anyhow::anyhow!("Invalid sample rate: {}", sample_rate_hz))?;
        debug!(
            "Running polyvoice diarization on {} samples at {}Hz",
            audio_samples.len(),
            sample_rate_hz
        );

        // This is a blocking operation, we should wrap it in spawn_blocking if it blocks the thread
        let result = pipeline.run(audio_samples, sample_rate)?;

        Ok(result)
    }
}

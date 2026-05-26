use anyhow::Result;
use async_trait::async_trait;
use tokio::sync::mpsc;

#[derive(Clone, Copy, Debug)]
pub struct AudioConfig {
    pub sample_rate_hz: u32,
    pub channels: u16,
    pub frame_size: usize,
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            sample_rate_hz: 16_000,
            channels: 1,
            frame_size: 512,
        }
    }
}

#[derive(Debug, Clone)]
pub struct AudioFrame {
    pub samples: Vec<f32>,
    pub timestamp_ms: u64,
}

#[async_trait]
pub trait AudioCapture: Send + Sync {
    async fn start(&mut self, config: AudioConfig) -> Result<mpsc::Receiver<AudioFrame>>;
    async fn stop(&mut self) -> Result<()>;
    fn is_capturing(&self) -> bool;
}

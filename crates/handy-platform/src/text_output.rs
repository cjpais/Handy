use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum OutputMode {
    Clipboard,
    Typed,
    ImeCommit,
}

#[async_trait]
pub trait TextOutput: Send + Sync {
    async fn deliver(&self, text: &str, mode: OutputMode) -> Result<()>;
}

use anyhow::Result;
use std::fs;
use std::path::Path;

use crate::exporters::srt::SubtitleChunk;

fn format_timestamp(ms: i64) -> String {
    let total_ms = ms.max(0);
    let hours = total_ms / 3_600_000;
    let minutes = (total_ms % 3_600_000) / 60_000;
    let seconds = (total_ms % 60_000) / 1000;
    let millis = total_ms % 1000;
    format!("{hours:02}:{minutes:02}:{seconds:02}.{millis:03}")
}

pub fn write(path: &Path, chunks: &[SubtitleChunk]) -> Result<()> {
    let body = chunks
        .iter()
        .filter(|chunk| !chunk.text.trim().is_empty())
        .map(|chunk| {
            format!(
                "{} --> {}\n{}\n",
                format_timestamp(chunk.start_ms),
                format_timestamp(chunk.end_ms.max(chunk.start_ms + 500)),
                chunk.text.trim()
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    fs::write(path, format!("WEBVTT\n\n{body}"))?;
    Ok(())
}

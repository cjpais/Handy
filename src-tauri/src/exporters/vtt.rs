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

#[cfg(test)]
mod tests {
    use super::write;
    use crate::exporters::srt::SubtitleChunk;

    #[test]
    fn write_includes_header_and_skips_empty_chunks() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let path = temp_dir.path().join("out.vtt");

        write(
            &path,
            &[
                SubtitleChunk {
                    start_ms: 0,
                    end_ms: 1_000,
                    text: "First".to_string(),
                },
                SubtitleChunk {
                    start_ms: 1_000,
                    end_ms: 2_000,
                    text: "   ".to_string(),
                },
                SubtitleChunk {
                    start_ms: 3_723_456,
                    end_ms: 3_724_000,
                    text: "Third".to_string(),
                },
            ],
        )
        .expect("vtt write should succeed");

        let output = std::fs::read_to_string(path).expect("read vtt");
        assert!(output.starts_with("WEBVTT\n\n"));
        assert!(output.contains("00:00:00.000 --> 00:00:01.000\nFirst"));
        assert!(output.contains("01:02:03.456 --> 01:02:04.000\nThird"));
        assert!(!output.contains("\n   \n"));
    }
}

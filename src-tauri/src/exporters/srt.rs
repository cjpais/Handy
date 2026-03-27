use anyhow::Result;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct SubtitleChunk {
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
}

fn format_timestamp(ms: i64) -> String {
    let total_ms = ms.max(0);
    let hours = total_ms / 3_600_000;
    let minutes = (total_ms % 3_600_000) / 60_000;
    let seconds = (total_ms % 60_000) / 1000;
    let millis = total_ms % 1000;
    format!("{hours:02}:{minutes:02}:{seconds:02},{millis:03}")
}

pub fn write(path: &Path, chunks: &[SubtitleChunk]) -> Result<()> {
    let body = chunks
        .iter()
        .filter(|chunk| !chunk.text.trim().is_empty())
        .enumerate()
        .map(|(index, chunk)| {
            format!(
                "{}\n{} --> {}\n{}\n",
                index + 1,
                format_timestamp(chunk.start_ms),
                format_timestamp(chunk.end_ms.max(chunk.start_ms + 500)),
                chunk.text.trim()
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    fs::write(path, body)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{write, SubtitleChunk};

    #[test]
    fn write_uses_sequential_numbers_after_skipping_empty_chunks() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let path = temp_dir.path().join("out.srt");

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
                    start_ms: 2_000,
                    end_ms: 3_000,
                    text: "Third".to_string(),
                },
            ],
        )
        .expect("srt write should succeed");

        let output = std::fs::read_to_string(path).expect("read srt");
        assert!(output.contains("1\n00:00:00,000 --> 00:00:01,000\nFirst"));
        assert!(output.contains("2\n00:00:02,000 --> 00:00:03,000\nThird"));
        assert!(!output.contains("\n3\n"));
    }
}

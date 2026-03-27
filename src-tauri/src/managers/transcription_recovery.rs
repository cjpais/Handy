use log::warn;
use std::fmt::Display;

#[derive(Clone, Debug, PartialEq)]
pub struct ChunkedTranscriptionResult {
    pub text: String,
    pub segments: Option<Vec<ChunkedTranscriptionSegment>>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ChunkedTranscriptionSegment {
    pub start: f32,
    pub end: f32,
    pub text: String,
}

#[derive(Clone, Copy)]
pub struct ChunkRetryPolicy<E> {
    pub label: &'static str,
    pub sample_rate_hz: usize,
    pub max_split_depth: usize,
    pub min_chunk_samples: usize,
    pub split_padding_samples: usize,
    pub max_merge_word_overlap: usize,
    pub should_retry: fn(&E) -> bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AudioSplit {
    midpoint_samples: usize,
    left_end_samples: usize,
    right_start_samples: usize,
    shared_overlap_samples: usize,
}

pub fn transcribe_with_chunk_retry<F, E>(
    audio: &[f32],
    policy: &ChunkRetryPolicy<E>,
    transcribe: &mut F,
) -> Result<ChunkedTranscriptionResult, E>
where
    F: FnMut(&[f32]) -> Result<ChunkedTranscriptionResult, E>,
    E: Display,
{
    transcribe_chunk(audio, policy, transcribe, 0)
}

fn transcribe_chunk<F, E>(
    audio: &[f32],
    policy: &ChunkRetryPolicy<E>,
    transcribe: &mut F,
    depth: usize,
) -> Result<ChunkedTranscriptionResult, E>
where
    F: FnMut(&[f32]) -> Result<ChunkedTranscriptionResult, E>,
    E: Display,
{
    match transcribe(audio) {
        Ok(result) => Ok(result),
        Err(error) => {
            if !(policy.should_retry)(&error) {
                return Err(error);
            }
            if depth >= policy.max_split_depth || !can_split(audio.len(), policy) {
                warn!(
                    "{} chunk recovery exhausted at depth {} for {:.2}s chunk: {}",
                    policy.label,
                    depth,
                    audio.len() as f32 / policy.sample_rate_hz as f32,
                    error
                );
                return Err(error);
            }

            let Some(split) = split_audio(audio.len(), policy.split_padding_samples) else {
                return Err(error);
            };

            warn!(
                "{} recoverable inference error at depth {}. Retrying with chunks of {:.2}s and {:.2}s",
                policy.label,
                depth,
                split.left_end_samples as f32 / policy.sample_rate_hz as f32,
                (audio.len() - split.right_start_samples) as f32 / policy.sample_rate_hz as f32,
            );

            let left_result = transcribe_chunk(
                &audio[..split.left_end_samples],
                policy,
                transcribe,
                depth + 1,
            )?;
            let right_result = transcribe_chunk(
                &audio[split.right_start_samples..],
                policy,
                transcribe,
                depth + 1,
            )?;

            Ok(merge_transcription_results(
                left_result,
                right_result,
                split,
                policy,
            ))
        }
    }
}

fn can_split<E>(audio_len: usize, policy: &ChunkRetryPolicy<E>) -> bool {
    (audio_len / 2) >= policy.min_chunk_samples
}

fn split_audio(audio_len: usize, split_padding_samples: usize) -> Option<AudioSplit> {
    if audio_len < 2 {
        return None;
    }

    let midpoint_samples = audio_len / 2;
    let padding_samples = if audio_len >= split_padding_samples * 4 {
        split_padding_samples
    } else {
        0
    };

    let left_end_samples = (midpoint_samples + padding_samples).min(audio_len);
    let right_start_samples = midpoint_samples.saturating_sub(padding_samples);

    if left_end_samples == 0 || right_start_samples >= audio_len {
        return None;
    }

    Some(AudioSplit {
        midpoint_samples,
        left_end_samples,
        right_start_samples,
        shared_overlap_samples: left_end_samples.saturating_sub(right_start_samples),
    })
}

fn merge_transcription_results<E>(
    left: ChunkedTranscriptionResult,
    right: ChunkedTranscriptionResult,
    split: AudioSplit,
    policy: &ChunkRetryPolicy<E>,
) -> ChunkedTranscriptionResult {
    let ChunkedTranscriptionResult {
        text: left_text,
        segments: left_segments,
    } = left;
    let ChunkedTranscriptionResult {
        text: right_text,
        segments: right_segments,
    } = right;

    match (left_segments, right_segments) {
        (Some(left_segments), Some(right_segments)) => {
            let merged_segments =
                merge_segment_results(left_segments, right_segments, split, policy.sample_rate_hz);

            if merged_segments.is_empty() {
                let merged_text = merge_transcription_text(
                    &left_text,
                    &right_text,
                    split.shared_overlap_samples > 0,
                    policy.max_merge_word_overlap,
                );
                return ChunkedTranscriptionResult {
                    text: merged_text,
                    segments: None,
                };
            }

            ChunkedTranscriptionResult {
                text: segments_to_text(&merged_segments),
                segments: Some(merged_segments),
            }
        }
        _ => ChunkedTranscriptionResult {
            text: merge_transcription_text(
                &left_text,
                &right_text,
                split.shared_overlap_samples > 0,
                policy.max_merge_word_overlap,
            ),
            segments: None,
        },
    }
}

fn merge_segment_results(
    left_segments: Vec<ChunkedTranscriptionSegment>,
    right_segments: Vec<ChunkedTranscriptionSegment>,
    split: AudioSplit,
    sample_rate_hz: usize,
) -> Vec<ChunkedTranscriptionSegment> {
    let seam_time = split.midpoint_samples as f32 / sample_rate_hz as f32;
    let right_offset_seconds = split.right_start_samples as f32 / sample_rate_hz as f32;

    let mut merged_segments = left_segments
        .into_iter()
        .filter(|segment| segment_midpoint(segment) < seam_time)
        .collect::<Vec<_>>();

    merged_segments.extend(
        right_segments
            .into_iter()
            .map(|segment| ChunkedTranscriptionSegment {
                start: segment.start + right_offset_seconds,
                end: segment.end + right_offset_seconds,
                text: segment.text,
            })
            .filter(|segment| segment_midpoint(segment) >= seam_time),
    );

    merged_segments
}

fn segment_midpoint(segment: &ChunkedTranscriptionSegment) -> f32 {
    (segment.start + segment.end) / 2.0
}

fn segments_to_text(segments: &[ChunkedTranscriptionSegment]) -> String {
    segments
        .iter()
        .map(|segment| segment.text.trim())
        .filter(|text: &&str| !text.is_empty())
        .map(|text| text.to_string())
        .collect::<Vec<_>>()
        .join(" ")
}

fn merge_transcription_text(
    left: &str,
    right: &str,
    has_overlap: bool,
    max_merge_word_overlap: usize,
) -> String {
    let left_trimmed = left.trim();
    let right_trimmed = right.trim();

    if left_trimmed.is_empty() {
        return right_trimmed.to_string();
    }
    if right_trimmed.is_empty() {
        return left_trimmed.to_string();
    }
    if !has_overlap {
        return format!("{} {}", left_trimmed, right_trimmed);
    }

    let left_words: Vec<&str> = left_trimmed.split_whitespace().collect();
    let right_words: Vec<&str> = right_trimmed.split_whitespace().collect();
    let max_overlap = left_words
        .len()
        .min(right_words.len())
        .min(max_merge_word_overlap);

    let overlap_words = (1..=max_overlap)
        .rev()
        .find(|&len| {
            let left_suffix = &left_words[left_words.len() - len..];
            let right_prefix = &right_words[..len];
            left_suffix
                .iter()
                .map(|word| normalize_overlap_word(word))
                .eq(right_prefix.iter().map(|word| normalize_overlap_word(word)))
        })
        .unwrap_or(0);

    if overlap_words == 0 {
        return format!("{} {}", left_trimmed, right_trimmed);
    }

    let mut merged_words = left_words
        .iter()
        .map(|word| (*word).to_string())
        .collect::<Vec<_>>();
    merged_words.extend(
        right_words[overlap_words..]
            .iter()
            .map(|word| (*word).to_string()),
    );
    merged_words.join(" ")
}

fn normalize_overlap_word(word: &str) -> String {
    let normalized = word
        .trim_matches(|character: char| !character.is_alphanumeric())
        .to_lowercase();

    if normalized.is_empty() {
        word.to_lowercase()
    } else {
        normalized
    }
}

#[cfg(test)]
mod tests {
    use super::{
        merge_segment_results, merge_transcription_text, split_audio, transcribe_with_chunk_retry,
        AudioSplit, ChunkRetryPolicy, ChunkedTranscriptionResult, ChunkedTranscriptionSegment,
    };
    use std::cell::Cell;
    use std::fmt::{Display, Formatter};

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum TestError {
        Retryable,
        Fatal,
    }

    impl Display for TestError {
        fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
            match self {
                Self::Retryable => formatter.write_str("retryable"),
                Self::Fatal => formatter.write_str("fatal"),
            }
        }
    }

    fn test_policy() -> ChunkRetryPolicy<TestError> {
        ChunkRetryPolicy {
            label: "test",
            sample_rate_hz: 4,
            max_split_depth: 8,
            min_chunk_samples: 4,
            split_padding_samples: 0,
            max_merge_word_overlap: 12,
            should_retry: |error| matches!(error, TestError::Retryable),
        }
    }

    #[test]
    fn merge_transcription_text_dedupes_overlap_words() {
        let merged = merge_transcription_text(
            "hello there general kenobi",
            "general kenobi you are a bold one",
            true,
            12,
        );

        assert_eq!(merged, "hello there general kenobi you are a bold one");
    }

    #[test]
    fn merge_transcription_text_preserves_repeated_words_without_overlap() {
        let merged = merge_transcription_text("no", "no more", false, 12);

        assert_eq!(merged, "no no more");
    }

    #[test]
    fn split_audio_adds_overlap_only_for_large_chunks() {
        let split = split_audio(96, 12).expect("chunk should split");

        assert_eq!(
            split,
            AudioSplit {
                midpoint_samples: 48,
                left_end_samples: 60,
                right_start_samples: 36,
                shared_overlap_samples: 24,
            }
        );

        let short_split = split_audio(24, 12).expect("chunk should split");
        assert_eq!(short_split.shared_overlap_samples, 0);
    }

    #[test]
    fn merge_segment_results_uses_seam_boundary() {
        let split = AudioSplit {
            midpoint_samples: 8,
            left_end_samples: 12,
            right_start_samples: 4,
            shared_overlap_samples: 8,
        };
        let merged = merge_segment_results(
            vec![
                ChunkedTranscriptionSegment {
                    start: 0.0,
                    end: 1.0,
                    text: "left".to_string(),
                },
                ChunkedTranscriptionSegment {
                    start: 1.0,
                    end: 3.0,
                    text: "shared".to_string(),
                },
            ],
            vec![
                ChunkedTranscriptionSegment {
                    start: 0.0,
                    end: 2.0,
                    text: "shared".to_string(),
                },
                ChunkedTranscriptionSegment {
                    start: 2.0,
                    end: 3.0,
                    text: "right".to_string(),
                },
            ],
            split,
            4,
        );

        assert_eq!(merged.len(), 3);
        assert_eq!(merged[0].text, "left");
        assert_eq!(merged[1].text, "shared");
        assert_eq!(merged[1].start, 1.0);
        assert_eq!(merged[1].end, 3.0);
        assert_eq!(merged[2].text, "right");
    }

    #[test]
    fn chunk_retry_recovers_retryable_failures() {
        let audio = (0..32).map(|sample| sample as f32).collect::<Vec<_>>();
        let calls = Cell::new(0);
        let mut transcribe = |chunk: &[f32]| {
            calls.set(calls.get() + 1);
            if chunk.len() > 8 {
                return Err(TestError::Retryable);
            }

            Ok(ChunkedTranscriptionResult {
                text: format!(
                    "{}-{}",
                    chunk.first().copied().unwrap_or_default() as i32,
                    chunk.last().copied().unwrap_or_default() as i32,
                ),
                segments: Some(vec![ChunkedTranscriptionSegment {
                    start: 0.0,
                    end: chunk.len() as f32 / 4.0,
                    text: format!(
                        "{}-{}",
                        chunk.first().copied().unwrap_or_default() as i32,
                        chunk.last().copied().unwrap_or_default() as i32,
                    ),
                }]),
            })
        };

        let result = transcribe_with_chunk_retry(&audio, &test_policy(), &mut transcribe)
            .expect("chunked retry should recover");

        assert_eq!(result.text, "0-7 8-15 16-23 24-31");
        assert_eq!(result.segments.unwrap().len(), 4);
        assert!(calls.get() > 1);
    }

    #[test]
    fn chunk_retry_fails_fast_for_non_retryable_errors() {
        let audio = vec![0.0; 32];
        let calls = Cell::new(0);
        let mut transcribe = |_chunk: &[f32]| {
            calls.set(calls.get() + 1);
            Err(TestError::Fatal)
        };

        let error = transcribe_with_chunk_retry(&audio, &test_policy(), &mut transcribe)
            .expect_err("non-retryable errors should not split");

        assert_eq!(error, TestError::Fatal);
        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn chunk_retry_stops_after_budget_is_exhausted() {
        let audio = vec![0.0; 32];
        let calls = Cell::new(0);
        let mut transcribe = |_chunk: &[f32]| {
            calls.set(calls.get() + 1);
            Err(TestError::Retryable)
        };

        let mut policy = test_policy();
        policy.max_split_depth = 1;

        let error = transcribe_with_chunk_retry(&audio, &policy, &mut transcribe)
            .expect_err("retry budget should be bounded");

        assert_eq!(error, TestError::Retryable);
        assert!((2..=3).contains(&calls.get()));
    }
}

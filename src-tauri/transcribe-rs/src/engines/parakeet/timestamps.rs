use super::engine::TimestampGranularity;
use super::model::TimestampedResult;
use crate::TranscriptionSegment;

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub text: String,
    pub token_id: Option<usize>,
    pub t_start: f32,
    pub t_end: f32,
    pub is_blank: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Word {
    pub text: String,
    pub t_start: f32,
    pub t_end: f32,
    pub tokens: Vec<Token>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Segment {
    pub text: String,
    pub t_start: f32,
    pub t_end: f32,
    pub words: Vec<Word>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Utterance {
    pub text: String,
    pub segments: Vec<Segment>,
}

// Legacy structs for backward compatibility
#[derive(Debug, Clone)]
pub struct WordBoundary {
    pub start: f32,
    pub end: f32,
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct TokenInfo {
    pub token: String,
    pub timestamp: f32,
    pub is_word_boundary: bool,
}

pub fn convert_timestamps(
    timestamped_result: &TimestampedResult,
    granularity: TimestampGranularity,
) -> Vec<TranscriptionSegment> {
    match granularity {
        TimestampGranularity::Token => convert_to_raw_token_segments(timestamped_result),
        TimestampGranularity::Word => convert_to_hierarchical_word_segments(timestamped_result),
        TimestampGranularity::Segment => {
            convert_to_hierarchical_segment_segments(timestamped_result)
        }
    }
}

// Returns the raw tokens exactly as output by the model
fn convert_to_raw_token_segments(
    timestamped_result: &TimestampedResult,
) -> Vec<TranscriptionSegment> {
    let mut segments = Vec::new();

    for (i, (token, &timestamp)) in timestamped_result
        .tokens
        .iter()
        .zip(timestamped_result.timestamps.iter())
        .enumerate()
    {
        // Include ALL tokens, even empty ones, for debugging purposes
        let end_timestamp = timestamped_result
            .timestamps
            .get(i + 1)
            .copied()
            .unwrap_or(timestamp + 0.05); // Small default duration for tokens

        segments.push(TranscriptionSegment {
            start: timestamp,
            end: end_timestamp,
            text: token.clone(), // Raw token text, including spaces and subword pieces
        });
    }

    segments
}

// Uses hierarchical approach for clean word-level timestamps
fn convert_to_hierarchical_word_segments(
    timestamped_result: &TimestampedResult,
) -> Vec<TranscriptionSegment> {
    let segment_separators = ['.', '?', '!'];
    let word_separator = ' ';

    let utterance =
        build_utterance_from_tokens(timestamped_result, &segment_separators, word_separator);

    extract_word_segments(&utterance)
}

// Uses hierarchical approach for clean segment-level timestamps
fn convert_to_hierarchical_segment_segments(
    timestamped_result: &TimestampedResult,
) -> Vec<TranscriptionSegment> {
    let segment_separators = ['.', '?', '!'];
    let word_separator = ' ';

    let utterance =
        build_utterance_from_tokens(timestamped_result, &segment_separators, word_separator);

    extract_segment_segments(&utterance)
}

fn build_utterance_from_tokens(
    timestamped_result: &TimestampedResult,
    segment_separators: &[char],
    word_separator: char,
) -> Utterance {
    // Handle empty input
    if timestamped_result.tokens.is_empty() || timestamped_result.timestamps.is_empty() {
        return Utterance {
            text: timestamped_result.text.clone(),
            segments: if timestamped_result.text.trim().is_empty() {
                Vec::new()
            } else {
                vec![Segment {
                    text: timestamped_result.text.clone(),
                    t_start: 0.0,
                    t_end: 0.0,
                    words: Vec::new(),
                }]
            },
        };
    }

    // Step 1: Create tokens from the timestamped result
    let tokens = create_tokens_from_timestamped_result(timestamped_result);

    // Step 2: Group tokens into words
    let words = group_tokens_into_words_hierarchical(&tokens, word_separator);

    // Step 3: Group words into segments
    let segments = group_words_into_segments(&words, segment_separators);

    Utterance {
        text: timestamped_result.text.clone(),
        segments,
    }
}

fn create_tokens_from_timestamped_result(timestamped_result: &TimestampedResult) -> Vec<Token> {
    let mut tokens = Vec::new();

    for (i, (token_text, &timestamp)) in timestamped_result
        .tokens
        .iter()
        .zip(timestamped_result.timestamps.iter())
        .enumerate()
    {
        let t_end = timestamped_result
            .timestamps
            .get(i + 1)
            .copied()
            .unwrap_or(timestamp + 0.05); // Small default duration for final token

        tokens.push(Token {
            text: token_text.clone(),
            token_id: Some(i),
            t_start: timestamp,
            t_end,
            is_blank: token_text.trim().is_empty(),
        });
    }

    tokens
}

fn group_tokens_into_words_hierarchical(tokens: &[Token], word_separator: char) -> Vec<Word> {
    let mut words = Vec::new();
    let mut current_word_tokens = Vec::new();

    for token in tokens {
        if token.is_blank {
            continue;
        }

        // Check if this token starts a new word
        // This handles subword tokens (like from tokenizers) and space-separated tokens
        let starts_new_word = token.text.starts_with(word_separator) ||
                             token.text.starts_with("▁") || // SentencePiece word boundary
                             (current_word_tokens.is_empty() && !token.text.trim().is_empty());

        if starts_new_word && !current_word_tokens.is_empty() {
            // Finish the current word
            let word = create_word_from_tokens(&current_word_tokens);
            if !word.text.is_empty() {
                words.push(word);
            }
            current_word_tokens.clear();
        }

        current_word_tokens.push(token.clone());
    }

    // Add the final word if there are tokens
    if !current_word_tokens.is_empty() {
        let word = create_word_from_tokens(&current_word_tokens);
        if !word.text.is_empty() {
            words.push(word);
        }
    }

    words
}

fn create_word_from_tokens(tokens: &[Token]) -> Word {
    if tokens.is_empty() {
        return Word {
            text: String::new(),
            t_start: 0.0,
            t_end: 0.0,
            tokens: Vec::new(),
        };
    }

    // Aggregate timestamps: word start = first token start, word end = last token end
    let t_start = tokens.first().unwrap().t_start;
    let t_end = tokens.last().unwrap().t_end;

    // Combine token text, handling subword tokens properly
    let text = tokens
        .iter()
        .map(|t| {
            // Handle SentencePiece tokens that start with ▁
            if t.text.starts_with("▁") {
                t.text.strip_prefix("▁").unwrap_or(&t.text)
            } else if t.text.starts_with(' ') {
                t.text.strip_prefix(' ').unwrap_or(&t.text)
            } else {
                &t.text
            }
        })
        .collect::<String>()
        .trim()
        .to_string();

    Word {
        text,
        t_start,
        t_end,
        tokens: tokens.to_vec(),
    }
}

fn group_words_into_segments(words: &[Word], segment_separators: &[char]) -> Vec<Segment> {
    if words.is_empty() {
        return Vec::new();
    }

    let mut segments = Vec::new();
    let mut current_segment_words = Vec::new();

    for (i, word) in words.iter().enumerate() {
        current_segment_words.push(word.clone());

        // Check if word ends with segment separator or if it's the last word
        let ends_segment =
            word.text.chars().any(|c| segment_separators.contains(&c)) || i == words.len() - 1; // Always end on last word

        if ends_segment {
            let segment = create_segment_from_words(&current_segment_words);
            if !segment.text.is_empty() {
                segments.push(segment);
            }
            current_segment_words.clear();
        }
    }

    // Handle case where no punctuation was found - create one big segment
    if segments.is_empty() && !words.is_empty() {
        let segment = create_segment_from_words(words);
        if !segment.text.is_empty() {
            segments.push(segment);
        }
    }

    segments
}

fn create_segment_from_words(words: &[Word]) -> Segment {
    if words.is_empty() {
        return Segment {
            text: String::new(),
            t_start: 0.0,
            t_end: 0.0,
            words: Vec::new(),
        };
    }

    // Aggregate timestamps: segment start = first word start, segment end = last word end
    let t_start = words.first().unwrap().t_start;
    let t_end = words.last().unwrap().t_end;

    // Combine word text with spaces
    let text = words
        .iter()
        .map(|w| w.text.as_str())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" ");

    Segment {
        text,
        t_start,
        t_end,
        words: words.to_vec(),
    }
}

fn extract_word_segments(utterance: &Utterance) -> Vec<TranscriptionSegment> {
    let mut segments = Vec::new();

    for segment in &utterance.segments {
        for word in &segment.words {
            if !word.text.trim().is_empty() {
                segments.push(TranscriptionSegment {
                    start: word.t_start,
                    end: word.t_end,
                    text: word.text.clone(),
                });
            }
        }
    }

    segments
}

fn extract_segment_segments(utterance: &Utterance) -> Vec<TranscriptionSegment> {
    utterance
        .segments
        .iter()
        .filter(|segment| !segment.text.trim().is_empty())
        .map(|segment| TranscriptionSegment {
            start: segment.t_start,
            end: segment.t_end,
            text: segment.text.clone(),
        })
        .collect()
}

// Legacy functions for backward compatibility
pub fn group_tokens_into_words(timestamped_result: &TimestampedResult) -> Vec<WordBoundary> {
    let words = convert_to_hierarchical_word_segments(timestamped_result);

    words
        .into_iter()
        .map(|segment| WordBoundary {
            start: segment.start,
            end: segment.end,
            text: segment.text,
        })
        .collect()
}

use anyhow::Result;
use chrono::Utc;
use log::{debug, info, warn};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::path::PathBuf;

use tauri::AppHandle;

#[derive(Clone, Debug, Serialize, Deserialize, Type)]
pub struct LearnedCorrection {
    pub id: i64,
    pub original_word: String,
    pub corrected_word: String,
    pub count: i64,
    pub created_at: i64,
    pub last_used_at: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize, Type)]
pub struct CorrectionDiff {
    pub original_word: String,
    pub corrected_word: String,
}

pub struct CorrectionsManager {
    db_path: PathBuf,
}

impl CorrectionsManager {
    pub fn new(app_handle: &AppHandle) -> Result<Self> {
        let app_data_dir = crate::portable::app_data_dir(app_handle)?;
        let db_path = app_data_dir.join("history.db");

        Ok(Self { db_path })
    }

    fn get_connection(&self) -> Result<rusqlite::Connection> {
        Ok(rusqlite::Connection::open(&self.db_path)?)
    }

    pub fn get_all_corrections(&self) -> Result<Vec<LearnedCorrection>> {
        let conn = self.get_connection()?;
        let mut stmt = conn.prepare(
            "SELECT id, original_word, corrected_word, count, created_at, last_used_at
             FROM learned_corrections
             ORDER BY count DESC, last_used_at DESC",
        )?;

        let corrections = stmt
            .query_map([], |row| {
                Ok(LearnedCorrection {
                    id: row.get("id")?,
                    original_word: row.get("original_word")?,
                    corrected_word: row.get("corrected_word")?,
                    count: row.get("count")?,
                    created_at: row.get("created_at")?,
                    last_used_at: row.get("last_used_at")?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(corrections)
    }

    pub fn learn_from_diff(
        &self,
        original_text: &str,
        corrected_text: &str,
    ) -> Result<Vec<CorrectionDiff>> {
        let diffs = Self::extract_word_diffs(original_text, corrected_text);

        if diffs.is_empty() {
            debug!("No word-level corrections found in diff");
            return Ok(vec![]);
        }

        let conn = self.get_connection()?;
        let now = Utc::now().timestamp();

        for diff in &diffs {
            let existing: Option<(i64, i64)> = conn
                .query_row(
                    "SELECT id, count FROM learned_corrections WHERE original_word = ?1",
                    params![diff.original_word],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()?;

            match existing {
                Some((id, count)) => {
                    conn.execute(
                        "UPDATE learned_corrections SET corrected_word = ?1, count = ?2, last_used_at = ?3 WHERE id = ?4",
                        params![diff.corrected_word, count + 1, now, id],
                    )?;
                    debug!(
                        "Updated correction: '{}' -> '{}' (count: {})",
                        diff.original_word, diff.corrected_word, count + 1
                    );
                }
                None => {
                    conn.execute(
                        "INSERT INTO learned_corrections (original_word, corrected_word, count, created_at, last_used_at) VALUES (?1, ?2, 1, ?3, ?4)",
                        params![diff.original_word, diff.corrected_word, now, now],
                    )?;
                    debug!(
                        "New correction learned: '{}' -> '{}'",
                        diff.original_word, diff.corrected_word
                    );
                }
            }
        }

        info!("Learned {} correction(s)", diffs.len());
        Ok(diffs)
    }

    pub async fn learn_from_diff_llm(
        &self,
        app: &AppHandle,
        original_text: &str,
        corrected_text: &str,
    ) -> Result<Vec<CorrectionDiff>> {
        let settings = crate::settings::get_settings(app);

        let provider = match settings.active_post_process_provider().cloned() {
            Some(p) => p,
            None => {
                debug!("No LLM provider configured, falling back to positional diff");
                return self.learn_from_diff(original_text, corrected_text);
            }
        };

        let model = settings
            .post_process_models
            .get(&provider.id)
            .cloned()
            .unwrap_or_default();

        if model.trim().is_empty() {
            debug!("No model configured for provider '{}', falling back", provider.id);
            return self.learn_from_diff(original_text, corrected_text);
        }

        let api_key = settings
            .post_process_api_keys
            .get(&provider.id)
            .cloned()
            .unwrap_or_default();

        if api_key.trim().is_empty() {
            debug!("No API key for provider '{}', falling back", provider.id);
            return self.learn_from_diff(original_text, corrected_text);
        }

        let system_prompt = "You are a speech-to-text error analyzer. Compare the original transcription with the corrected version. Extract ONLY word-level corrections that fix speech recognition errors (misheard words, wrong homophones, phonetic mistakes). Ignore changes in punctuation, capitalization, style, or added/removed words. Return a JSON object with a \"corrections\" array of objects, each with \"wrong\" and \"correct\" keys. If there are no STT errors to fix, return {\"corrections\": []}".to_string();

        let user_content = format!(
            "ORIGINAL: {}\nCORRECTED: {}",
            original_text, corrected_text
        );

        let json_schema = serde_json::json!({
            "type": "object",
            "properties": {
                "corrections": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "wrong": { "type": "string" },
                            "correct": { "type": "string" }
                        },
                        "required": ["wrong", "correct"],
                        "additionalProperties": false
                    }
                }
            },
            "required": ["corrections"],
            "additionalProperties": false
        });

        let (reasoning_effort, reasoning) = match provider.id.as_str() {
            "custom" => (Some("none".to_string()), None),
            "openrouter" => (
                None,
                Some(crate::llm_client::ReasoningConfig {
                    effort: Some("none".to_string()),
                    exclude: Some(true),
                }),
            ),
            _ => (None, None),
        };

        let use_schema = provider.supports_structured_output;

        let result = if use_schema {
            crate::llm_client::send_chat_completion_with_schema(
                &provider,
                api_key,
                &model,
                user_content,
                Some(system_prompt),
                Some(json_schema),
                reasoning_effort,
                reasoning,
            )
            .await
        } else {
            let combined = format!("{}\n\n{}", system_prompt, user_content);
            crate::llm_client::send_chat_completion(
                &provider,
                api_key,
                &model,
                combined,
                reasoning_effort,
                reasoning,
            )
            .await
        };

        let response_text = match result {
            Ok(Some(text)) => text,
            Ok(None) => {
                debug!("LLM returned empty response, falling back");
                return self.learn_from_diff(original_text, corrected_text);
            }
            Err(e) => {
                warn!("LLM correction analysis failed: {}, falling back", e);
                return self.learn_from_diff(original_text, corrected_text);
            }
        };

        let diffs = match Self::parse_llm_corrections(&response_text) {
            Some(d) if !d.is_empty() => d,
            _ => {
                debug!("LLM returned no corrections, falling back");
                return self.learn_from_diff(original_text, corrected_text);
            }
        };

        info!("LLM extracted {} correction(s)", diffs.len());
        self.save_diffs_to_db(&diffs)?;
        Ok(diffs)
    }

    fn parse_llm_corrections(response: &str) -> Option<Vec<CorrectionDiff>> {
        let cleaned = response.trim();

        #[derive(Deserialize)]
        struct LlmCorrection {
            wrong: String,
            correct: String,
        }

        #[derive(Deserialize)]
        struct LlmResponse {
            corrections: Vec<LlmCorrection>,
        }

        let parsed: LlmResponse = serde_json::from_str(cleaned).ok()?;

        Some(
            parsed
                .corrections
                .into_iter()
                .filter(|c| !c.wrong.trim().is_empty() && !c.correct.trim().is_empty())
                .map(|c| CorrectionDiff {
                    original_word: c.wrong.trim().to_lowercase(),
                    corrected_word: c.correct.trim().to_string(),
                })
                .collect(),
        )
    }

    fn save_diffs_to_db(&self, diffs: &[CorrectionDiff]) -> Result<()> {
        let conn = self.get_connection()?;
        let now = Utc::now().timestamp();

        for diff in diffs {
            let existing: Option<(i64, i64)> = conn
                .query_row(
                    "SELECT id, count FROM learned_corrections WHERE original_word = ?1",
                    params![diff.original_word],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()?;

            match existing {
                Some((id, count)) => {
                    conn.execute(
                        "UPDATE learned_corrections SET corrected_word = ?1, count = ?2, last_used_at = ?3 WHERE id = ?4",
                        params![diff.corrected_word, count + 1, now, id],
                    )?;
                }
                None => {
                    conn.execute(
                        "INSERT INTO learned_corrections (original_word, corrected_word, count, created_at, last_used_at) VALUES (?1, ?2, 1, ?3, ?4)",
                        params![diff.original_word, diff.corrected_word, now, now],
                    )?;
                }
            }
        }

        Ok(())
    }

    pub fn apply_corrections(&self, text: &str) -> Result<String> {
        let corrections = self.get_all_corrections()?;
        if corrections.is_empty() {
            return Ok(text.to_string());
        }

        let mut result = text.to_string();
        for correction in &corrections {
            let pattern = regex::Regex::new(&format!(
                r"(?i)\b{}\b",
                regex::escape(&correction.original_word)
            ))
            .ok();

            if let Some(re) = pattern {
                let original_len = result.len();
                result = re
                    .replace_all(&result, correction.corrected_word.as_str())
                    .to_string();
                if result.len() != original_len || result != text {
                    debug!(
                        "Applied correction: '{}' -> '{}'",
                        correction.original_word, correction.corrected_word
                    );
                }
            }
        }

        Ok(result)
    }

    pub fn delete_correction(&self, id: i64) -> Result<()> {
        let conn = self.get_connection()?;
        conn.execute(
            "DELETE FROM learned_corrections WHERE id = ?1",
            params![id],
        )?;
        info!("Deleted correction with id {}", id);
        Ok(())
    }

    pub fn clear_all_corrections(&self) -> Result<usize> {
        let conn = self.get_connection()?;
        let count = conn.execute("DELETE FROM learned_corrections", [])?;
        info!("Cleared all corrections ({} deleted)", count);
        Ok(count)
    }

    fn extract_word_diffs(original: &str, corrected: &str) -> Vec<CorrectionDiff> {
        let original_words: Vec<&str> = original.split_whitespace().collect();
        let corrected_words: Vec<&str> = corrected.split_whitespace().collect();

        let mut diffs = Vec::new();

        let max_len = original_words.len().max(corrected_words.len());
        for i in 0..max_len {
            let orig_word = original_words.get(i).map(|s| *s).unwrap_or("");
            let corr_word = corrected_words.get(i).map(|s| *s).unwrap_or("");

            if !orig_word.is_empty()
                && !corr_word.is_empty()
                && !Self::words_match_ignore_case_punctuation(orig_word, corr_word)
            {
                let clean_orig = Self::strip_punctuation(orig_word);
                let clean_corr = Self::strip_punctuation(corr_word);

                if !clean_orig.is_empty() && !clean_corr.is_empty() {
                    diffs.push(CorrectionDiff {
                        original_word: clean_orig.to_lowercase(),
                        corrected_word: clean_corr,
                    });
                }
            }
        }

        diffs
    }

    fn words_match_ignore_case_punctuation(a: &str, b: &str) -> bool {
        let clean_a = Self::strip_punctuation(a).to_lowercase();
        let clean_b = Self::strip_punctuation(b).to_lowercase();
        clean_a == clean_b
    }

    fn strip_punctuation(s: &str) -> String {
        s.chars()
            .filter(|c| c.is_alphanumeric() || *c == '\'' || *c == '-')
            .collect()
    }
}

//! Word statistics: which words, openers and phrases the owner dictates, filler
//! words and their trend, and new words.
//!
//! Layout: [`analyze`] turns a text into stems, forms and n-grams (the tokenizer
//! of the basic statistics is reused), [`stopwords`] holds the RU + EN lists,
//! [`index`] keeps the daily aggregates in step with the dictations table
//! (record, remove, one-time backfill, cleanup) and [`query`] answers the screen's
//! questions from those aggregates.

pub mod analyze;
pub mod index;
pub mod query;
pub mod stopwords;
#[cfg(test)]
mod tests;

/// Event with the progress of the one-time indexing of older dictations.
pub const BACKFILL_EVENT: &str = "voxa-words-backfill";

use serde::Deserialize;
use specta::Type;

/// Local calendar days (`YYYY-MM-DD`), both ends inclusive; `None` is open.
#[derive(Debug, Clone, Default, Deserialize, Type)]
pub struct WordsPeriod {
    pub from: Option<String>,
    pub to: Option<String>,
}

impl WordsPeriod {
    /// No bounds at all (all time).
    pub fn is_open(&self) -> bool {
        self.from.is_none() && self.to.is_none()
    }

    /// Bounds usable in `day BETWEEN ?1 AND ?2` (days sort as text).
    pub fn bounds(&self) -> (String, String) {
        (
            self.from.clone().unwrap_or_else(|| "0000-00-00".to_string()),
            self.to.clone().unwrap_or_else(|| "9999-99-99".to_string()),
        )
    }
}

use serde::{Deserialize, Serialize};

/// How long unsaved recordings are kept before automatic cleanup.
///
/// `PreserveLimit` uses a count-based limit (configured separately via `HistoryLimit`).
/// All other variants use a duration-based cleanup.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RecordingRetentionPeriod {
    Never,
    PreserveLimit,
    Days3,
    Weeks2,
    Months3,
}

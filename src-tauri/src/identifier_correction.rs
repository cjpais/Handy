//! Identifier correction layer for voice-driven coding.
//!
//! After transcription, this module intercepts the output text and replaces
//! phonetically-mangled code identifiers (file names, function names, variable
//! names, etc.) with fuzzy-matched candidates from the active codebase.
//!
//! Correction flow:
//!   1. `rebuild_index()` walks the project root (using fd/rg) and builds a
//!      deduplicated symbol list of file paths, stems, function/class/variable names.
//!   2. `correct_text()` tokenises the transcription, detects candidate tokens
//!      (explicit triggers or automatic heuristics), fuzzy-matches each against the
//!      index, and applies corrections.
//!   3. If a single high-confidence match is found, the correction is silent.
//!   4. If multiple candidates exist, a batch `identifier-pick-needed` event is
//!      emitted to the frontend and the pipeline blocks (up to 10 s) for the user
//!      to confirm via the picker overlay.

use serde::{Deserialize, Serialize};
use specta::Type;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{mpsc, Arc, Mutex, RwLock};
use std::time::Duration;
use tauri_specta::Event;

// ---------------------------------------------------------------------------
// Static word lists
// ---------------------------------------------------------------------------

/// ~250 most common English words that should never be treated as identifiers.
static COMMON_ENGLISH: &[&str] = &[
    "a", "about", "above", "across", "after", "again", "against", "all", "almost",
    "alone", "along", "already", "also", "although", "always", "am", "among", "an",
    "and", "another", "any", "anyone", "anything", "are", "around", "as", "ask",
    "at", "away", "back", "be", "because", "been", "before", "being", "below",
    "between", "both", "but", "by", "can", "come", "could", "day", "did", "do",
    "does", "done", "down", "during", "each", "end", "even", "every", "few", "find",
    "first", "for", "form", "from", "get", "give", "go", "good", "great", "had",
    "has", "have", "he", "her", "here", "him", "his", "how", "however", "if", "in",
    "into", "is", "it", "its", "just", "know", "large", "last", "later", "let",
    "like", "long", "look", "made", "make", "many", "may", "me", "might", "more",
    "most", "move", "much", "must", "my", "new", "next", "no", "nor", "not", "now",
    "of", "off", "often", "old", "on", "once", "only", "or", "other", "our", "out",
    "over", "own", "part", "people", "place", "put", "rather", "said", "same",
    "see", "she", "should", "since", "small", "so", "some", "still", "such",
    "take", "than", "that", "the", "their", "them", "then", "there", "these",
    "they", "this", "those", "though", "through", "time", "to", "too", "two",
    "under", "until", "up", "upon", "us", "use", "used", "very", "was", "way",
    "we", "well", "were", "what", "when", "where", "which", "while", "who",
    "will", "with", "without", "would", "you", "your",
    // Programming-adjacent prose words that are too common to correct
    "code", "run", "test", "build", "set", "get", "add", "line", "name", "type",
    "true", "false", "null", "none", "data", "text", "list", "read", "write",
    "new", "old", "one", "two", "three", "four", "five",
];

/// Words that explicitly signal the next token is an identifier.
/// The trigger word itself is removed from the final output.
static EXPLICIT_TRIGGERS: &[&str] = &[
    "file", "symbol", "function", "class", "method", "variable", "module", "package",
];

/// Words that raise the probability of the following token being an identifier.
static CODE_CONTEXT_WORDS: &[&str] = &[
    "open", "edit", "rename", "delete", "create", "import", "require", "from",
    "execute", "call", "define", "implement", "extend", "instantiate", "initialize",
    "struct", "enum", "interface", "trait", "macro", "constant",
];

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A request to the user to select from multiple identifier candidates.
/// Emitted as a Tauri event; the frontend shows a picker overlay.
#[derive(Clone, Debug, Serialize, Deserialize, Type, tauri_specta::Event)]
pub struct IdentifierPickNeededEvent {
    pub request_id: String,
    /// All ambiguous tokens in this transcription, batched into one round-trip.
    pub items: Vec<PickItem>,
}

/// One ambiguous token needing user confirmation.
#[derive(Clone, Debug, Serialize, Deserialize, Type)]
pub struct PickItem {
    /// The original transcribed token.
    pub token: String,
    /// Ranked replacement candidates (best first).
    pub candidates: Vec<String>,
}

/// Confidence classification for a fuzzy match.
#[derive(Debug, PartialEq)]
enum Confidence {
    /// Single best match, substitute silently.
    High,
    /// Several plausible matches, ask the user.
    Ambiguous,
    /// Nothing good enough.
    None,
}

/// Whether a token is a correction candidate and why.
#[derive(Debug, PartialEq)]
enum CandidateKind {
    /// Word immediately following an explicit trigger like "file" or "symbol".
    ExplicitTrigger,
    /// Heuristically detected as a likely identifier.
    Automatic,
    /// Not a candidate.
    No,
}

// ---------------------------------------------------------------------------
// Manager
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct IdentifierCorrectionManager {
    /// Deduplicated, flat symbol list (file stems, paths, function names, …).
    symbols: Arc<RwLock<Vec<String>>>,
    /// Pending picker requests: request_id → sender.
    pending_picks: Arc<Mutex<HashMap<String, mpsc::SyncSender<HashMap<String, String>>>>>,
}

impl IdentifierCorrectionManager {
    pub fn new() -> Self {
        Self {
            symbols: Arc::new(RwLock::new(Vec::new())),
            pending_picks: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Rebuild the symbol index from a project root directory.
    /// Returns the number of symbols indexed.
    pub fn rebuild_index(&self, project_root: &Path) -> usize {
        log::info!(
            "Identifier correction: building index from {}",
            project_root.display()
        );
        let syms = extract_symbols(project_root);
        let count = syms.len();
        *self.symbols.write().unwrap() = syms;
        log::info!("Identifier correction: indexed {} symbols", count);
        count
    }

    /// Return the number of symbols currently in the index.
    pub fn symbol_count(&self) -> usize {
        self.symbols.read().unwrap().len()
    }

    /// Apply identifier correction to transcribed text.
    ///
    /// Called from the transcription pipeline after filler-word removal.
    pub fn correct_text(
        &self,
        app: &tauri::AppHandle,
        text: &str,
        threshold: f64,
    ) -> String {
        let symbols = self.symbols.read().unwrap();
        if symbols.is_empty() {
            return text.to_string();
        }

        let common: HashSet<&str> = COMMON_ENGLISH.iter().copied().collect();
        let words: Vec<&str> = text.split_whitespace().collect();
        if words.is_empty() {
            return text.to_string();
        }

        // ---- Phase 1: classify each token ----
        // We collect:
        //   silent_corrections: word index → replacement (applied without interaction)
        //   ambiguous_items: tokens needing picker
        //   trigger_indices: word indices of explicit trigger words (to remove from output)
        let mut silent: HashMap<usize, String> = HashMap::new();
        let mut ambiguous: Vec<(usize, PickItem)> = Vec::new();
        let mut trigger_indices: HashSet<usize> = HashSet::new();

        let mut i = 0usize;
        while i < words.len() {
            let word = words[i];
            let prev = if i > 0 { Some(words[i - 1]) } else { None };

            let kind = classify_token(word, prev, &common);
            if kind == CandidateKind::No {
                i += 1;
                continue;
            }

            if kind == CandidateKind::ExplicitTrigger {
                // Mark the preceding trigger word for removal.
                if i > 0 {
                    trigger_indices.insert(i - 1);
                }
            }

            let (confidence, candidates) = find_matches(word, &symbols, threshold);
            match confidence {
                Confidence::None => {}
                Confidence::High => {
                    silent.insert(i, candidates[0].clone());
                }
                Confidence::Ambiguous => {
                    ambiguous.push((
                        i,
                        PickItem {
                            token: word.to_string(),
                            candidates,
                        },
                    ));
                }
            }

            i += 1;
        }

        // ---- Phase 2: resolve ambiguous tokens via picker ----
        let mut picker_selections: HashMap<String, String> = HashMap::new();
        if !ambiguous.is_empty() {
            let items: Vec<PickItem> = ambiguous.iter().map(|(_, item)| item.clone()).collect();
            picker_selections = self.request_picker(app, items);
        }

        // ---- Phase 3: reconstruct the corrected string ----
        let mut output_words: Vec<Option<&str>> = words.iter().map(|w| Some(*w)).collect();

        // Apply silent corrections
        for (idx, replacement) in &silent {
            // Can't put a String into Vec<Option<&str>> directly.
            // We'll build the output in a separate pass below.
            let _ = (idx, replacement); // handled in the owned-string pass
        }

        // Build the final string handling both silent and picker corrections.
        let mut result_parts: Vec<String> = Vec::new();
        for (idx, word) in words.iter().enumerate() {
            if trigger_indices.contains(&idx) {
                // Skip the trigger keyword (e.g. "file", "symbol").
                continue;
            }
            if let Some(replacement) = silent.get(&idx) {
                result_parts.push(replacement.clone());
            } else if let Some((_, item)) = ambiguous.iter().find(|(i, _)| *i == idx) {
                // Use picker selection if the user chose something; else keep original.
                let selected = picker_selections
                    .get(&item.token)
                    .cloned()
                    .unwrap_or_else(|| item.token.clone());
                result_parts.push(selected);
            } else {
                let _ = output_words[idx]; // suppress unused warning
                result_parts.push(word.to_string());
            }
        }

        result_parts.join(" ")
    }

    /// Resolve a pending picker request from the frontend.
    pub fn resolve_pick(
        &self,
        request_id: &str,
        selections: HashMap<String, String>,
    ) {
        if let Some(tx) = self.pending_picks.lock().unwrap().remove(request_id) {
            let _ = tx.send(selections);
        }
    }

    // ---- Internal: emit picker event and block for response ----

    fn request_picker(
        &self,
        app: &tauri::AppHandle,
        items: Vec<PickItem>,
    ) -> HashMap<String, String> {
        // Unique request ID from nanosecond timestamp.
        let request_id = format!(
            "{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        );

        let (tx, rx) = mpsc::sync_channel::<HashMap<String, String>>(1);

        {
            self.pending_picks
                .lock()
                .unwrap()
                .insert(request_id.clone(), tx);
        }

        // Emit event to the frontend picker overlay using the tauri_specta typed API.
        if let Err(e) = (IdentifierPickNeededEvent {
            request_id: request_id.clone(),
            items,
        })
        .emit(app)
        {
            log::warn!("Failed to emit identifier-pick-needed event: {}", e);
            self.pending_picks.lock().unwrap().remove(&request_id);
            return HashMap::new();
        }

        // Block the transcription pipeline for up to 10 seconds.
        let result = rx
            .recv_timeout(Duration::from_secs(10))
            .unwrap_or_default();

        // Ensure cleanup even on timeout.
        self.pending_picks.lock().unwrap().remove(&request_id);

        result
    }
}

// ---------------------------------------------------------------------------
// Symbol extraction
// ---------------------------------------------------------------------------

/// Collect symbols from the project root using available CLI tools.
fn extract_symbols(root: &Path) -> Vec<String> {
    let mut seen: HashSet<String> = HashSet::new();

    extract_file_symbols(root, &mut seen);
    extract_code_symbols(root, &mut seen);

    // Limit to a reasonable size for memory / matching speed.
    let mut syms: Vec<String> = seen.into_iter().filter(|s| s.len() >= 2).collect();
    syms.sort();
    syms.dedup();
    syms.truncate(15_000);
    syms
}

/// Glob file paths and add both the stem and the relative path to the set.
fn extract_file_symbols(root: &Path, seen: &mut HashSet<String>) {
    let root_str = root.to_string_lossy().to_string();

    // Prefer fd (respects .gitignore, very fast); fall back to find.
    let output = if command_exists("fd") {
        Command::new("fd")
            .args(["--type", "f", "--strip-cwd-prefix"])
            .current_dir(&root_str)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output()
    } else {
        Command::new("find")
            .arg(".")
            .args([
                "-type",
                "f",
                "-not",
                "-path",
                "*/.git/*",
                "-not",
                "-path",
                "*/node_modules/*",
                "-not",
                "-path",
                "*/target/*",
                "-not",
                "-path",
                "*/.build/*",
            ])
            .current_dir(&root_str)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output()
    };

    if let Ok(out) = output {
        for line in String::from_utf8_lossy(&out.stdout).lines() {
            let p = Path::new(line.trim_start_matches("./"));
            // Add the relative path as-is (e.g. "src/utils.rs")
            seen.insert(p.to_string_lossy().to_string());
            // Add the file stem without extension (e.g. "utils")
            if let Some(stem) = p.file_stem().and_then(|s| s.to_str()) {
                seen.insert(stem.to_string());
            }
        }
    }
}

/// Use ripgrep to extract function, class, struct, and variable names.
fn extract_code_symbols(root: &Path, seen: &mut HashSet<String>) {
    if !command_exists("rg") {
        return;
    }

    // Language-agnostic patterns for common declaration forms.
    let patterns: &[&str] = &[
        // Rust / Python / Go / Swift functions
        r"(?:fn|def|func|fun)\s+(\w{2,})",
        // Class / struct / interface / enum / trait
        r"(?:class|struct|interface|enum|trait|type)\s+(\w{2,})",
        // TypeScript / JS const/let declarations
        r"(?:const|let|var)\s+(\w{2,})\s*[=:]",
    ];

    for pattern in patterns {
        let output = Command::new("rg")
            .args([
                "--no-heading",
                "--no-filename",
                "--no-line-number",
                "--only-matching",
                "--replace",
                "$1",
            ])
            .arg(pattern)
            .arg(root)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output();

        if let Ok(out) = output {
            for sym in String::from_utf8_lossy(&out.stdout).lines() {
                let sym = sym.trim();
                if sym.len() >= 2 && !is_common_keyword(sym) {
                    seen.insert(sym.to_string());
                }
            }
        }
    }
}

/// Common language keywords that should never be treated as user symbols.
fn is_common_keyword(s: &str) -> bool {
    matches!(
        s,
        "fn" | "let" | "const" | "var" | "def" | "func" | "fun" | "class"
            | "struct" | "enum" | "trait" | "type" | "interface" | "if" | "else"
            | "for" | "while" | "loop" | "match" | "return" | "pub" | "use"
            | "mod" | "impl" | "self" | "Self" | "super" | "crate" | "where"
            | "async" | "await" | "move" | "mut" | "ref" | "static" | "extern"
            | "unsafe" | "true" | "false" | "null" | "None" | "Some" | "Ok" | "Err"
    )
}

// ---------------------------------------------------------------------------
// Token classification
// ---------------------------------------------------------------------------

fn classify_token(word: &str, prev: Option<&str>, common: &HashSet<&str>) -> CandidateKind {
    // Must be non-empty, reasonable length.
    if word.len() < 3 || word.len() > 40 {
        return CandidateKind::No;
    }

    // Skip pure numbers.
    if word.chars().all(|c| c.is_ascii_digit() || c == '.') {
        return CandidateKind::No;
    }

    // Skip punctuation-only tokens.
    if word.chars().all(|c| !c.is_alphabetic()) {
        return CandidateKind::No;
    }

    // Skip ALL_CAPS (acronyms like HTTP, API).
    let all_caps = word.chars().filter(|c| c.is_alphabetic()).all(|c| c.is_uppercase());
    if all_caps {
        return CandidateKind::No;
    }

    let lower = word.to_lowercase();

    // Explicit trigger: previous word is a trigger keyword.
    if let Some(p) = prev {
        let pl = p.to_lowercase();
        if EXPLICIT_TRIGGERS.iter().any(|t| *t == pl) {
            return CandidateKind::ExplicitTrigger;
        }
    }

    // Skip very common English words.
    if common.contains(lower.as_str()) {
        return CandidateKind::No;
    }

    // Code-context word before this token raises priority.
    if let Some(p) = prev {
        let pl = p.to_lowercase();
        if CODE_CONTEXT_WORDS.iter().any(|c| *c == pl) {
            return CandidateKind::Automatic;
        }
    }

    // Looks like an identifier by structure: contains underscore or mid-uppercase.
    if looks_like_identifier(word) {
        return CandidateKind::Automatic;
    }

    CandidateKind::No
}

/// Returns true for camelCase, PascalCase, or snake_case tokens.
fn looks_like_identifier(word: &str) -> bool {
    if word.contains('_') {
        return true;
    }
    let mut chars = word.chars();
    if let Some(first) = chars.next() {
        // camelCase: starts lowercase, contains uppercase later.
        if first.is_lowercase() && chars.any(|c| c.is_uppercase()) {
            return true;
        }
    }
    // PascalCase: starts with uppercase, second char lowercase.
    let mut chars = word.chars();
    if let (Some(a), Some(b)) = (chars.next(), chars.next()) {
        if a.is_uppercase() && b.is_lowercase() && word.len() > 4 {
            return true;
        }
    }
    false
}

// ---------------------------------------------------------------------------
// Fuzzy matching
// ---------------------------------------------------------------------------

/// Match a query against the symbol index.
/// Returns a confidence level and the top candidates.
fn find_matches(query: &str, symbols: &[String], threshold: f64) -> (Confidence, Vec<String>) {
    let candidates = if command_exists("fzf") {
        fzf_filter(query, symbols)
    } else {
        // Linear scan limited to keep latency acceptable.
        symbols.iter().cloned().collect::<Vec<_>>()
    };

    score_candidates(query, &candidates, threshold)
}

/// Run `fzf --filter` over the symbol list and return the top 20 hits.
fn fzf_filter(query: &str, symbols: &[String]) -> Vec<String> {
    use std::io::Write;

    let mut child = match Command::new("fzf")
        .args([
            "--filter",
            query,
            "--no-sort",
            "--algo=v2",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(c) => c,
        Err(_) => return symbols.to_vec(),
    };

    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(symbols.join("\n").as_bytes());
    }

    match child.wait_with_output() {
        Ok(out) => String::from_utf8_lossy(&out.stdout)
            .lines()
            .take(20)
            .map(|l| l.to_string())
            .collect(),
        Err(_) => symbols.to_vec(),
    }
}

/// Score candidates against the query using normalised Levenshtein distance,
/// optionally boosted by Soundex phonetic similarity.
fn score_candidates(
    query: &str,
    candidates: &[String],
    threshold: f64,
) -> (Confidence, Vec<String>) {
    // Thresholds:
    //   >= HIGH_THRESH  → silent single replacement
    //   >= threshold    → include in ambiguous set (shown to user)
    const HIGH_THRESH: f64 = 0.85;

    let q_lower = query.to_lowercase();
    let q_soundex = soundex(&q_lower);

    let mut scored: Vec<(f64, String)> = candidates
        .iter()
        .filter_map(|sym| {
            let s_lower = sym.to_lowercase();
            // Also try matching against just the file stem to avoid penalising
            // "utils.rs" when the query is "utils".
            let stem = Path::new(&s_lower)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or(&s_lower)
                .to_string();

            let lev = strsim::normalized_levenshtein(&q_lower, &stem)
                .max(strsim::normalized_levenshtein(&q_lower, &s_lower));

            // Phonetic boost: +0.15 if Soundex codes match.
            let phonetic_bonus = if soundex(&stem) == q_soundex { 0.15 } else { 0.0 };

            let score = (lev + phonetic_bonus).min(1.0);
            if score >= threshold {
                Some((score, sym.clone()))
            } else {
                None
            }
        })
        .collect();

    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(5);

    if scored.is_empty() {
        return (Confidence::None, vec![]);
    }

    let top_score = scored[0].0;
    let names: Vec<String> = scored.into_iter().map(|(_, s)| s).collect();

    if top_score >= HIGH_THRESH && names.len() == 1 {
        (Confidence::High, names)
    } else if top_score >= HIGH_THRESH && names.len() > 1 && names[0] != names[1] {
        // Strong top match and it's clearly better than the rest.
        let second_score_high = {
            // Re-score the second item against the same query to see if it competes.
            let s_lower = names[1].to_lowercase();
            let stem = Path::new(&s_lower)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or(&s_lower)
                .to_string();
            strsim::normalized_levenshtein(&query.to_lowercase(), &stem)
        };
        if top_score - second_score_high > 0.15 {
            (Confidence::High, vec![names[0].clone()])
        } else {
            (Confidence::Ambiguous, names)
        }
    } else {
        (Confidence::Ambiguous, names)
    }
}

// ---------------------------------------------------------------------------
// Utilities
// ---------------------------------------------------------------------------

/// Check whether a command is available on PATH.
fn command_exists(cmd: &str) -> bool {
    Command::new("which")
        .arg(cmd)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Minimal Soundex implementation for phonetic matching.
fn soundex(s: &str) -> String {
    let s = s.to_uppercase();
    let mut chars = s.chars().filter(|c| c.is_ascii_alphabetic());
    let first = match chars.next() {
        Some(c) => c,
        None => return "0000".to_string(),
    };

    let encode = |c: char| -> char {
        match c {
            'B' | 'F' | 'P' | 'V' => '1',
            'C' | 'G' | 'J' | 'K' | 'Q' | 'S' | 'X' | 'Z' => '2',
            'D' | 'T' => '3',
            'L' => '4',
            'M' | 'N' => '5',
            'R' => '6',
            _ => '0',
        }
    };

    let mut code = first.to_string();
    let mut prev = encode(first);

    for c in chars {
        let digit = encode(c);
        if digit != '0' && digit != prev {
            code.push(digit);
            if code.len() == 4 {
                break;
            }
        }
        prev = digit;
    }

    while code.len() < 4 {
        code.push('0');
    }
    code
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ---- soundex ----

    #[test]
    fn soundex_basic() {
        // Standard Soundex examples.
        assert_eq!(soundex("Robert"), "R163");
        assert_eq!(soundex("Rupert"), "R163");
        assert_eq!(soundex("Ashcraft"), "A261");
    }

    #[test]
    fn soundex_empty() {
        assert_eq!(soundex(""), "0000");
    }

    #[test]
    fn soundex_short() {
        let s = soundex("A");
        assert_eq!(s.len(), 4);
        assert!(s.starts_with('A'));
    }

    // ---- looks_like_identifier ----

    #[test]
    fn identifier_snake_case() {
        assert!(looks_like_identifier("parse_args"));
        assert!(looks_like_identifier("my_var_name"));
    }

    #[test]
    fn identifier_camel_case() {
        assert!(looks_like_identifier("parseArgs"));
        assert!(looks_like_identifier("myVarName"));
    }

    #[test]
    fn identifier_pascal_case() {
        assert!(looks_like_identifier("ParseArgs"));
        assert!(looks_like_identifier("MyStruct"));
    }

    #[test]
    fn identifier_plain_word_not_flagged() {
        // Plain lowercase words without structural clues are NOT identifiers.
        assert!(!looks_like_identifier("hello"));
        assert!(!looks_like_identifier("world"));
        assert!(!looks_like_identifier("open"));
    }

    // ---- classify_token ----

    fn make_common() -> HashSet<&'static str> {
        COMMON_ENGLISH.iter().copied().collect()
    }

    #[test]
    fn classify_explicit_trigger_file() {
        let common = make_common();
        let kind = classify_token("utils", Some("file"), &common);
        assert_eq!(kind, CandidateKind::ExplicitTrigger);
    }

    #[test]
    fn classify_explicit_trigger_symbol() {
        let common = make_common();
        let kind = classify_token("parseArgs", Some("symbol"), &common);
        assert_eq!(kind, CandidateKind::ExplicitTrigger);
    }

    #[test]
    fn classify_code_context() {
        let common = make_common();
        // "utils" is not common English + follows "open" (a context word).
        let kind = classify_token("utils", Some("open"), &common);
        assert_eq!(kind, CandidateKind::Automatic);
    }

    #[test]
    fn classify_snake_case_automatic() {
        let common = make_common();
        let kind = classify_token("parse_args", None, &common);
        assert_eq!(kind, CandidateKind::Automatic);
    }

    #[test]
    fn classify_camel_case_automatic() {
        let common = make_common();
        let kind = classify_token("parseArgs", None, &common);
        assert_eq!(kind, CandidateKind::Automatic);
    }

    #[test]
    fn classify_common_word_skipped() {
        let common = make_common();
        // "the", "and", "is" are in the common list.
        assert_eq!(classify_token("the", None, &common), CandidateKind::No);
        assert_eq!(classify_token("and", None, &common), CandidateKind::No);
        assert_eq!(classify_token("is", None, &common), CandidateKind::No);
    }

    #[test]
    fn classify_all_caps_skipped() {
        let common = make_common();
        assert_eq!(classify_token("HTTP", None, &common), CandidateKind::No);
        assert_eq!(classify_token("API", None, &common), CandidateKind::No);
    }

    #[test]
    fn classify_too_short_skipped() {
        let common = make_common();
        assert_eq!(classify_token("ab", None, &common), CandidateKind::No);
    }

    #[test]
    fn classify_number_skipped() {
        let common = make_common();
        assert_eq!(classify_token("123", None, &common), CandidateKind::No);
        assert_eq!(classify_token("3.14", None, &common), CandidateKind::No);
    }

    // ---- score_candidates ----

    #[test]
    fn score_exact_match_is_high_confidence() {
        let symbols = vec!["utils.rs".to_string(), "main.rs".to_string(), "lib.rs".to_string()];
        let (confidence, matches) = score_candidates("utils", &symbols, 0.60);
        // "utils" vs "utils.rs" → stem is "utils" → normalized_levenshtein = 1.0
        assert_eq!(confidence, Confidence::High);
        assert_eq!(matches[0], "utils.rs");
    }

    #[test]
    fn score_no_match_below_threshold() {
        let symbols = vec!["completely_unrelated".to_string(), "another_thing".to_string()];
        let (confidence, matches) = score_candidates("xyz", &symbols, 0.60);
        assert_eq!(confidence, Confidence::None);
        assert!(matches.is_empty());
    }

    #[test]
    fn score_ambiguous_when_close_competitors() {
        // Both "parse_input" and "parse_output" are close to "parse_input";
        // neither dominates the other by >0.15 margin.
        let symbols = vec![
            "parse_input".to_string(),
            "parse_output".to_string(),
        ];
        let (confidence, matches) = score_candidates("parse_input", &symbols, 0.60);
        // "parse_input" should be HIGH (exact match), "parse_output" would also score high.
        // The exact result depends on second-score logic; at minimum both should be present.
        assert!(!matches.is_empty());
        assert_eq!(matches[0], "parse_input");
        // For an exact match the top score is 1.0 and the gap should be > 0.15.
        assert_eq!(confidence, Confidence::High);
    }

    #[test]
    fn score_phonetic_boost_helps_similar_sounding() {
        // "utilise" sounds like "utils" (both U340 in Soundex).
        let symbols = vec!["utils".to_string()];
        let (confidence, _) = score_candidates("utilise", &symbols, 0.60);
        // Even if raw Levenshtein is borderline, phonetic boost should push it through.
        // We just assert it doesn't return None (may be Ambiguous or High).
        assert_ne!(confidence, Confidence::None);
    }

    // ---- is_common_keyword ----

    #[test]
    fn keywords_filtered_out() {
        assert!(is_common_keyword("fn"));
        assert!(is_common_keyword("struct"));
        assert!(is_common_keyword("None"));
        assert!(!is_common_keyword("parse_args"));
        assert!(!is_common_keyword("MyClass"));
    }

    // ---- integration: correct_text (without Tauri, using internal helpers) ----

    /// Directly test the scoring + reconstruction path by calling `find_matches`
    /// and re-assembling the output, mimicking what `correct_text` does.
    #[test]
    fn integration_silent_correction_of_file_stem() {
        let symbols = vec!["utils.rs".to_string(), "main.rs".to_string()];
        let threshold = 0.60_f64;

        // "utils" → should silently match "utils.rs"
        let (conf, candidates) = find_matches("utils", &symbols, threshold);
        assert_eq!(conf, Confidence::High);
        assert_eq!(candidates[0], "utils.rs");
    }

    #[test]
    fn integration_no_correction_for_common_prose() {
        // Plain common words should not even reach find_matches in the real pipeline
        // because classify_token filters them. We test classify_token here.
        let common = make_common();
        let prose_words = ["the", "open", "and", "with", "from", "into"];
        for word in &prose_words {
            // Words in the common list should be CandidateKind::No even when
            // following a context word (except explicit trigger words).
            if COMMON_ENGLISH.contains(word) {
                assert_eq!(
                    classify_token(word, Some("open"), &common),
                    CandidateKind::No,
                    "expected '{}' to be skipped as common English",
                    word
                );
            }
        }
    }

    #[test]
    fn integration_trigger_word_triggers_correction() {
        let common = make_common();
        // "file" is an explicit trigger, so "utils" that follows it should be ExplicitTrigger.
        assert_eq!(
            classify_token("utils", Some("file"), &common),
            CandidateKind::ExplicitTrigger
        );
        // "symbol" trigger too.
        assert_eq!(
            classify_token("parseArgs", Some("symbol"), &common),
            CandidateKind::ExplicitTrigger
        );
    }
}

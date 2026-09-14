/**
 * Dictionary data helpers (fork feature).
 *
 * Pure, dependency-free functions shared by the Dictionary settings UI and the
 * Playwright specs. No React and no Tauri imports live here so every function
 * can be exercised directly in tests.
 *
 * Two conceptually separate systems live on the Dictionary page:
 * - Custom words reuse Handy's existing `custom_words` setting and its fuzzy
 *   matcher (src-tauri/src/audio_toolkit/text.rs::apply_custom_words).
 * - Replacement rules are a deterministic, exact final-output correction layer
 *   (src-tauri/src/audio_toolkit/dictionary.rs::apply_dictionary_replacements).
 */

export interface DictionaryRule {
  from: string;
  to: string;
}

export interface DictionaryFileData {
  words: string[];
  replacements: DictionaryRule[];
}

/** Parsed file plus any recoverable issues found while parsing. */
export interface ParsedDictionaryFile extends DictionaryFileData {
  /** CSV rows that could not be interpreted as a rule (skipped, not applied). */
  malformedRows: number;
}

export type DictionaryParseErrorCode =
  | "invalidJson"
  | "invalidFormat"
  | "unsupportedVersion"
  | "malformedCsv";

export class DictionaryParseError extends Error {
  readonly code: DictionaryParseErrorCode;
  readonly detail?: string;

  constructor(code: DictionaryParseErrorCode, detail?: string) {
    super(detail ? `${code}: ${detail}` : code);
    this.name = "DictionaryParseError";
    this.code = code;
    this.detail = detail;
  }
}

/** Maximum length for a custom word, mirroring CustomWords.tsx. */
export const WORDS_MAX_LENGTH = 50;

/**
 * Mirrors the normalization applied to manually-added custom words in
 * CustomWords.tsx: strips characters that break the fuzzy matcher, collapses
 * whitespace runs, and trims.
 */
export function normalizeWord(word: string): string {
  return word
    .replace(/[<>"']/g, "")
    .replace(/\s+/g, " ")
    .trim();
}

/**
 * Normalization for entries that come from a file (import paths only): trim and
 * collapse whitespace runs, nothing else.
 *
 * `normalizeWord` additionally strips `[<>"']` for upstream CustomWords parity,
 * which would make an export → import round trip lossy ("CJ's Studio" would come
 * back as "CJs Studio"). Never apply it to stored data.
 */
export function normalizeImportedWord(word: string): string {
  return word.trim().split(/\s+/).join(" ");
}

/** Trim and collapse whitespace on a rule's "heard" side (casing is kept). */
export function normalizeReplacementFrom(from: string): string {
  return from.replace(/\s+/g, " ").trim();
}

/** Trim a rule's replacement side; internal spacing and casing are kept. */
export function normalizeReplacementTo(to: string): string {
  return to.trim();
}

export type WordValidationError = "empty" | "tooLong" | "duplicate";

export function validateWord(
  word: string,
  existing: string[],
): WordValidationError | null {
  const normalized = normalizeWord(word);
  if (!normalized) return "empty";
  if (normalized.length > WORDS_MAX_LENGTH) return "tooLong";
  const duplicate = existing.some(
    (entry) => entry.toLowerCase() === normalized.toLowerCase(),
  );
  return duplicate ? "duplicate" : null;
}

export type ReplacementValidationError = "emptyFrom" | "emptyTo" | "duplicate";

export function validateReplacement(
  from: string,
  to: string,
  existing: DictionaryRule[],
  ignoreIndex?: number,
): ReplacementValidationError | null {
  const normalizedFrom = normalizeReplacementFrom(from);
  const normalizedTo = normalizeReplacementTo(to);
  if (!normalizedFrom) return "emptyFrom";
  if (!normalizedTo) return "emptyTo";
  const duplicate = existing.some(
    (rule, index) =>
      index !== ignoreIndex &&
      normalizeReplacementFrom(rule.from).toLowerCase() ===
        normalizedFrom.toLowerCase(),
  );
  return duplicate ? "duplicate" : null;
}

/** Case-insensitive local search over custom words (never mutates input). */
export function filterWords(words: string[], query: string): string[] {
  const needle = query.trim().toLowerCase();
  if (!needle) return words;
  return words.filter((word) => word.toLowerCase().includes(needle));
}

/** Case-insensitive local search over rules, matching either side. */
export function filterReplacements(
  rules: DictionaryRule[],
  query: string,
): DictionaryRule[] {
  const needle = query.trim().toLowerCase();
  if (!needle) return rules;
  return rules.filter(
    (rule) =>
      rule.from.toLowerCase().includes(needle) ||
      rule.to.toLowerCase().includes(needle),
  );
}

/**
 * One entry per line; blank lines ignored; entries normalized for import only
 * (whitespace), so stored words such as "CJ's Studio" survive a TXT round trip.
 */
export function parseWordsTxt(content: string): string[] {
  return content
    .split(/\r?\n/)
    .map(normalizeImportedWord)
    .filter((word) => word.length > 0);
}

/**
 * Minimal RFC 4180-style CSV reader: comma separated, double quotes with `""`
 * escapes, and CRLF/LF line endings (newlines allowed inside quotes).
 * Rows that are not exactly two non-empty columns are skipped and counted.
 * Throws on an unterminated quoted field (the whole file is rejected so a
 * broken import can never be applied partially).
 */
export function parseReplacementsCsv(content: string): {
  rules: DictionaryRule[];
  malformedRows: number;
} {
  const rows: string[][] = [];
  let row: string[] = [];
  let field = "";
  let inQuotes = false;
  let index = 0;

  while (index < content.length) {
    const char = content[index];
    if (inQuotes) {
      if (char === '"') {
        if (content[index + 1] === '"') {
          field += '"';
          index += 2;
          continue;
        }
        inQuotes = false;
        index += 1;
        continue;
      }
      field += char;
      index += 1;
      continue;
    }
    if (char === '"') {
      inQuotes = true;
      index += 1;
      continue;
    }
    if (char === ",") {
      row.push(field);
      field = "";
      index += 1;
      continue;
    }
    if (char === "\n" || char === "\r") {
      if (char === "\r" && content[index + 1] === "\n") index += 1;
      row.push(field);
      field = "";
      rows.push(row);
      row = [];
      index += 1;
      continue;
    }
    field += char;
    index += 1;
  }
  if (inQuotes) {
    throw new DictionaryParseError("malformedCsv", "unterminated quoted field");
  }
  if (field.length > 0 || row.length > 0) {
    row.push(field);
    rows.push(row);
  }

  const rules: DictionaryRule[] = [];
  let malformedRows = 0;
  for (const columns of rows) {
    if (columns.length === 1 && columns[0].trim() === "") continue; // blank line
    if (columns.length !== 2) {
      malformedRows += 1;
      continue;
    }
    const from = columns[0].trim();
    const to = columns[1].trim();
    if (!from || !to) {
      malformedRows += 1;
      continue;
    }
    rules.push({ from, to });
  }

  // Tolerate the optional "from,to" header line written by the CSV export.
  if (
    rules.length > 0 &&
    rules[0].from.toLowerCase() === "from" &&
    rules[0].to.toLowerCase() === "to"
  ) {
    rules.shift();
  }

  return { rules, malformedRows };
}

/** Parse the portable JSON dictionary format produced by the export action. */
export function parseDictionaryJson(content: string): DictionaryFileData {
  let data: unknown;
  try {
    data = JSON.parse(content);
  } catch (error) {
    throw new DictionaryParseError(
      "invalidJson",
      error instanceof Error ? error.message : undefined,
    );
  }
  if (typeof data !== "object" || data === null || Array.isArray(data)) {
    throw new DictionaryParseError("invalidFormat");
  }
  const record = data as Record<string, unknown>;
  if (record.schema !== "handy-dictionary") {
    throw new DictionaryParseError("invalidFormat");
  }
  if (record.version !== 1) {
    throw new DictionaryParseError(
      "unsupportedVersion",
      String(record.version ?? "unknown"),
    );
  }
  const rawWords = record.customWords ?? [];
  const rawReplacements = record.replacements ?? [];
  if (
    !Array.isArray(rawWords) ||
    !rawWords.every((word) => typeof word === "string")
  ) {
    throw new DictionaryParseError("invalidFormat");
  }
  if (
    !Array.isArray(rawReplacements) ||
    !rawReplacements.every(
      (rule) =>
        typeof rule === "object" &&
        rule !== null &&
        typeof (rule as Record<string, unknown>).from === "string" &&
        typeof (rule as Record<string, unknown>).to === "string",
    )
  ) {
    throw new DictionaryParseError("invalidFormat");
  }
  return {
    words: rawWords
      .map(normalizeImportedWord)
      .filter((word) => word.length > 0),
    replacements: rawReplacements.map((rule) => ({
      from: (rule as DictionaryRule).from,
      to: (rule as DictionaryRule).to,
    })),
  };
}

export type DictionaryFileFormat = "json" | "txt" | "csv";

/**
 * Detect the format from the file extension, falling back to content sniffing
 * when the extension is unknown.
 */
export function detectDictionaryFormat(
  fileName: string,
  content: string,
): DictionaryFileFormat {
  const lower = fileName.toLowerCase();
  if (lower.endsWith(".json")) return "json";
  if (lower.endsWith(".csv")) return "csv";
  if (lower.endsWith(".txt")) return "txt";
  const trimmed = content.trimStart();
  if (trimmed.startsWith("{")) return "json";
  if (trimmed.toLowerCase().startsWith("from,to")) return "csv";
  return "txt";
}

/** Parse any supported dictionary file; rejects before anything is applied. */
export function parseDictionaryFile(
  fileName: string,
  content: string,
): ParsedDictionaryFile {
  const format = detectDictionaryFormat(fileName, content);
  if (format === "json") {
    return { ...parseDictionaryJson(content), malformedRows: 0 };
  }
  if (format === "csv") {
    const { rules, malformedRows } = parseReplacementsCsv(content);
    return { words: [], replacements: rules, malformedRows };
  }
  return { words: parseWordsTxt(content), replacements: [], malformedRows: 0 };
}

export interface MergeWordsResult {
  words: string[];
  added: number;
  /** Total entries that were not applied: `duplicates` + `invalid`. */
  skipped: number;
  /** Case-insensitive duplicates of an existing or already-imported entry. */
  duplicates: number;
  /** Blank or over-long entries (never applied, never blank-typed). */
  invalid: number;
}

/**
 * Merge imported words into the existing list: normalize for import (whitespace
 * only, `normalizeWord` is reserved for the manual-add path), drop blanks and
 * entries beyond the manual-add length, and skip (case-insensitive) duplicates.
 * Duplicates and invalid entries are reported separately so the caller can tell
 * the user which of the two actually happened.
 */
export function mergeWords(
  existing: string[],
  incoming: string[],
): MergeWordsResult {
  const words = [...existing];
  const seen = new Set(words.map((word) => word.toLowerCase()));
  let added = 0;
  let duplicates = 0;
  let invalid = 0;
  for (const raw of incoming) {
    const word = normalizeImportedWord(raw);
    if (!word || word.length > WORDS_MAX_LENGTH) {
      invalid += 1;
      continue;
    }
    const key = word.toLowerCase();
    if (seen.has(key)) {
      duplicates += 1;
      continue;
    }
    seen.add(key);
    words.push(word);
    added += 1;
  }
  return { words, added, skipped: duplicates + invalid, duplicates, invalid };
}

export interface MergeReplacementsResult {
  rules: DictionaryRule[];
  added: number;
  skipped: number;
  conflicts: number;
}

/**
 * Merge imported rules into the existing list (merge, never replace): blank
 * sides are skipped, duplicates keep the existing rule and are reported as
 * conflicts.
 */
export function mergeReplacements(
  existing: DictionaryRule[],
  incoming: DictionaryRule[],
): MergeReplacementsResult {
  const rules = existing.map((rule) => ({
    from: normalizeReplacementFrom(rule.from),
    to: normalizeReplacementTo(rule.to),
  }));
  const seen = new Set(rules.map((rule) => rule.from.toLowerCase()));
  let added = 0;
  let skipped = 0;
  let conflicts = 0;
  for (const raw of incoming) {
    const from = normalizeReplacementFrom(raw.from);
    const to = normalizeReplacementTo(raw.to);
    if (!from || !to) {
      skipped += 1;
      continue;
    }
    const key = from.toLowerCase();
    if (seen.has(key)) {
      conflicts += 1;
      continue;
    }
    seen.add(key);
    rules.push({ from, to });
    added += 1;
  }
  return { rules, added, skipped, conflicts };
}

/** Canonical, complete dictionary export (the only lossless format). */
export function buildDictionaryJson(
  words: string[],
  replacements: DictionaryRule[],
): string {
  return (
    JSON.stringify(
      {
        schema: "handy-dictionary",
        version: 1,
        customWords: words,
        replacements,
      },
      null,
      2,
    ) + "\n"
  );
}

export function buildWordsTxt(words: string[]): string {
  return words.length > 0 ? words.join("\n") + "\n" : "";
}

function escapeCsvField(value: string): string {
  return /[",\r\n]/.test(value) ? `"${value.replace(/"/g, '""')}"` : value;
}

export function serializeReplacementsCsv(
  replacements: DictionaryRule[],
): string {
  const lines = [
    "from,to",
    ...replacements.map(
      (rule) => `${escapeCsvField(rule.from)},${escapeCsvField(rule.to)}`,
    ),
  ];
  return lines.join("\n") + "\n";
}

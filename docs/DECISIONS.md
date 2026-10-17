# Decisions

Format: what was chosen, why, what the alternatives were. Newest decisions are
appended at the bottom.

## D-01 Working branch

- **Chosen:** all work on `claude/magical-ritchie-lg9k96` (the branch the cloud
  session was started on). The CI workflow triggers on pushes to `voxa/**`,
  `claude/**` and manually. The PR goes `claude/magical-ritchie-lg9k96 -> main`.
- **Why:** the session is configured to push only to its own branch; pushing to
  `voxa/main-work` would bypass that. The branch name carries no behaviour.
- **Alternatives:** create `voxa/main-work` as in PLAN.md (rejected, see above).

## D-02 Model format for "large-v3-turbo q8_0"

- **Chosen:** the catalog entry `handy-computer/whisper-large-v3-turbo-gguf`,
  file `whisper-large-v3-turbo-Q8_0.gguf` (886 MB), as the default model. It is
  already the catalog's default quant. It is flagged `recommended` and gets a
  "recommended for 8 GB Macs" hint. F16 (1.6 GB) stays available; GigaAM v3
  entries are untouched.
- **Why:** this fork loads models through `transcribe-cpp`, which only reads GGUF.
  `ggml-large-v3-turbo-q8_0.bin` from `ggerganov/whisper.cpp` is a different
  container and cannot be loaded here. The GGUF Q8_0 is the same quantization of
  the same weights (874 MB vs 886 MB, the difference is metadata).
- **Alternatives:** add a whisper.cpp `.bin` loader (a rewrite of the engine layer,
  out of scope); download the .bin anyway (the app would fail to load it).

## D-03 Auto-update

- **Chosen:** the updater is switched off completely: `update_checks_forced_disabled()`
  returns `true` in Voxa builds, the default of `update_checks_enabled` is `false`,
  `createUpdaterArtifacts` is `false` and the endpoint points at this repository's
  releases (inactive). No update request is ever made.
- **Why:** PLAN.md section 3 (an update from `cjpais/Handy` would replace the fork
  with the original) and section 8 (no network calls except explicit model
  downloads). Without a signing key CI cannot produce updater artifacts anyway,
  so `tauri build` would fail with `createUpdaterArtifacts: true`.
- **Alternatives:** own signed releases with a minisign key (needs a secret that
  does not exist, and a release process that is out of scope).

## D-04 Charts

- **Chosen:** a hand-written SVG bar chart (`src/voxa/stats/DailyBars.tsx`), no
  charting dependency.
- **Why:** 30 bars need no library; zero extra bytes in the bundle and RAM on an
  8 GB machine; full control over the glass look and `prefers-reduced-motion`.
- **Alternatives:** Recharts (about 100 kB gzip + d3 modules for one bar chart).

## D-05 Where dictation metrics live

- **Chosen:** a separate SQLite file `voxa_stats.db` in the app data directory
  (`com.daniyar.voxa`) with its own migration chain
  (`rusqlite_migration`, its own `user_version`). One row per dictation with
  every field from PLAN.md 5.1 (`duration_ms`, `transcribe_ms`, `model_id`,
  `language`, `raw_text`, `final_text`, `replacements_json`, word counts) plus
  `history_id`. On first start the table is backfilled from existing
  `history.db` rows (read only, word counts and timestamps).
- **Why:**
  1. Handy's history cleanup (`history_limit`, retention period) deletes rows and
     WAVs. A separate store cannot lose statistics to it, which is what 5.3
     requires. No change of Handy's cleanup semantics is needed.
  2. Upstream appends migrations to `history.rs` by position
     (`PRAGMA user_version`). Appending Voxa columns there would collide with the
     next upstream migration on every `git merge upstream/main`.
  3. The history UI and retry flow stay untouched.
- **Privacy:** text is stored locally only. The statistics screen has a
  "Clear statistics" action that deletes the file contents.
- **Alternatives:** `ALTER TABLE transcription_history` (PLAN.md's literal wording;
  rejected for the two reasons above, and it would force audio-only cleanup).

## D-06 Custom-word candidates

- **Chosen:** candidates come from (a) `replacements_json` frequency and (b) a
  built-in list of Cyrillic transliterations of technical terms matched by stem
  against final texts. The built-in list contains the 12 terms from PLAN.md and
  about 25 more (TypeScript, Kubernetes, Redis, ...). The button adds the Latin
  term to `custom_words`; it is disabled with "already in dictionary" when the
  term is present (case-insensitive).
- **Why:** all 12 terms from PLAN.md are in the owner's pre-filled dictionary, so
  with only those the button could never be enabled. The extra terms make it
  useful and testable.

## D-07 Frontend stack for the new screens

- **Chosen:** same stack as upstream (React, Tailwind 4, i18next). New strings are
  added to all 26 locale files because `check:translations` fails otherwise;
  English and Russian are written by hand, other locales get the English text
  (marked in the file by being identical to `en`).

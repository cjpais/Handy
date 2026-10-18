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

- **Chosen:** plain CSS bars (`src/voxa/stats/DailyBars.tsx`), no charting
  dependency.
- **Why:** 30 bars need no library; zero extra bytes in the bundle and RAM on an
  8 GB machine; full control over the glass look, hover readout and
  `prefers-reduced-motion`.
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

## D-08 Case repair for dictionary words the model already saw

- **Chosen:** after transcription, custom words are also applied when the model
  received them as a Whisper glossary, but with a threshold that only accepts
  words equal to a dictionary word once case and spacing are ignored
  (`langchain` -> `LangChain`, `Lang Chain` -> `LangChain`). A setting
  (`voxa.fix_custom_word_case`, default on) switches it off. Replacements it makes
  are reported like any other, so the statistics can show them.
- **Why:** upstream skips fuzzy matching whenever the vocabulary was sent to the
  model, and Whisper always gets it. Without this pass `replacements_json` would
  stay empty for the owner's setup and brand names would keep the wrong case.
  The fuzzy matcher only understands ASCII, so it cannot turn `питон` into
  `Python`; that case is covered by the transliteration candidates.
- **Alternatives:** leave upstream behaviour (no replacement data for Whisper);
  run the full fuzzy pass after prompting (risk of rewriting correct words).

## D-09 Context phrase for Whisper

- **Chosen:** the editable phrase goes into `RunOptions.prompt` (Whisper's context
  prompt, rendered after the `Glossary:` of the custom words in the same slot).
  Only models that advertise `Feature::InitialPrompt` (Whisper) get it.
- **Why:** Whisper's `initial_prompt` extension cannot be combined with the
  vocabulary the app already sends, the generic prompt can.

## D-10 Default model is chosen on the onboarding screen, not preset

- **Chosen:** `selected_model` stays empty on a fresh install. Whisper
  large-v3-turbo Q8_0 is catalog rank 1 and recommended, so it is the first card
  of the onboarding model step; the download starts on click.
- **Why:** `ModelManager::auto_select_model_if_needed` clears a selected model that
  is not on disk, so presetting it does nothing, and downloading without a click
  would break "no network except explicit downloads".

## D-11 Word classes in the statistics

- **Chosen:** every token falls into exactly one class: Latin (contains an ASCII
  letter, so `RAG-пайплайн` and `Claude'а` are Latin), else Cyrillic, else other
  (numbers, other scripts). `latin + cyrillic + other = words`. A token is a run
  of letters and digits where `-`, `'`, `’`, `_` and `.` between alphanumerics and
  `,` between digits join it (`GPT-4o`, `e-mail`, `3.11`, `3,14`); a chunk without
  a letter or digit is not a word.
- **Why:** it matches PLAN.md 5.2 and keeps the three counters consistent.

## D-12 Visual direction

- **Chosen:** after the owner's reference screenshot (dark, editorial): near-black
  canvas with a faint swirl, dark glass cards with hairline gradient rims, 24 pt
  card radius, serif display numerals (`ui-serif`, New York on macOS), round icon
  buttons, a gold highlight for "today", azure only for functional accents
  (switches, primary buttons, focus). The light appearance keeps the structure on
  a warm white canvas. The recording capsule is monochrome glass for the same
  reason.
- **Why:** the owner asked for "something like this" mid-task. Card radius is
  24 pt instead of the 14-20 pt in PLAN.md, following the reference.
- **How:** palette and radius tokens in `src/styles/theme.css` and
  `src/voxa/glass.css` (`@theme` radii, `--vg-*` pairs), `vg-` classes only on the
  few card containers (settings groups, history list, sidebar, footer).

## D-13 Overlay window geometry and clicks

- **Chosen:** on macOS the overlay window is exactly the capsule (220x48), the
  native HUD vibrancy view fills it with a 24 pt corner radius and the native
  window shadow gives the soft shadow. The Live (streaming) card keeps its larger
  window and runs without vibrancy. The window accepts clicks (needed for the
  hover cancel button); it is a non-activating panel, so it never takes focus.
- **Why:** vibrancy fills the whole window, so a bigger window would show a blurred
  rectangle around the capsule. Truly click-through except for the cancel button
  would need per-pixel hit testing driven by cursor polling, which cannot be
  verified without a Mac.
- **Risk:** if hover events do not reach a non-key panel, the cancel button stays
  invisible; Esc still cancels. Listed in the manual checklist.

## D-14 NSGlassEffectView (stretch goal) not shipped

- **Chosen:** vibrancy (`Effect::HudWindow`) plus CSS layers only.
- **Why:** `objc2-app-kit` has the bindings, but native glass would have to be
  written and iterated against a Mac. CI could not run (BLOCKERS B-03), so any
  compile or runtime mistake in that code would break the owner's build or the
  overlay without a way to see it. PLAN.md allows the fallback.
- **Next step:** enable the `NSGlassEffectView` feature of `objc2-app-kit`, create
  the view behind the webview when `NSClassFromString("NSGlassEffectView")` exists
  (macOS 26+), keep vibrancy otherwise.

## D-15 PLAN.md excluded from the Prettier check

- **Chosen:** `PLAN.md` is listed in `.prettierignore`.
- **Why:** it is the owner's file and CI runs `prettier --check .`.

## D-16 Strings in other locales

- **Chosen:** `en` and `ru` carry the new `voxa.*` strings and the renamed app
  name; the 24 other locales get the English `voxa.*` block (and
  "Based on Handy" instead of the donate row) so the translation check passes.
  `scripts/voxa/sync_locales.py` regenerates them from `src/voxa/i18n/`.

## D-17 In-app updates replace the "never update" rule of D-03

- **Chosen:** the updater is back, with these limits: `update_checks_forced_disabled()` is the upstream env switch again (`HANDY_DISABLE_UPDATER`); the check runs in the Rust backend (30 s after start, then every 6 hours) and only when "Check for updates automatically" is on (default on); installing happens only on the button. The one-time settings migration `voxa.updates_default_applied` switches the stored `update_checks_enabled` to on for stores written by the first Voxa build, which persisted `false`.
- **Why:** PLAN-2 part A. Checking in the backend keeps it working with the window closed and lets the tray menu show "Update to vX.Y.Z...". A failed background check never replaces what the owner already sees; only a manual check reports errors.
- **Privacy:** the feed (`latest.json` on this repository's releases) and the signed archive after the click are the only new network requests, besides model downloads.
- **Alternatives:** a JS-side check in the webview (stops with the window closed, and Handy's component also carries the Windows portable logic that is not needed here).

## D-18 Version scheme

- **Chosen:** `0.10.<workflow run number>` (`scripts/voxa/release_manifest.py`). The release workflow writes it to `tauri.conf.json` and `Cargo.toml` of the build only; nothing is committed back.
- **Why:** the updater offers a build only when its version is higher. The run number only grows; minor `10` is above the `0.9.8` of the CI test builds, so those update to the first release.
- **Re-runs:** a re-run of the same workflow run has the same version and replaces the assets of its release instead of failing.

## D-19 How a release is built and signed

- **Chosen:** `tauri build --bundles app` without updater artifacts, then the workflow signs the app itself (own certificate from the secrets, ad-hoc without them), packs `Voxa.app.tar.gz` and signs that with `tauri signer sign`, writes `latest.json` and publishes with `gh release`.
- **Why:** Tauri creates its archive during the build, before any signing of ours, so an archive made by `tauri build` would hold the wrong signature. Doing the steps by hand fixes the order and does not depend on a third-party action.
- **Signature check:** with a certificate the job fails unless `codesign -d -r-` shows `identifier "com.daniyar.voxa" and certificate leaf`; a `cdhash` requirement would reset macOS permissions at every update.
- **Public key:** injected from the secret `TAURI_UPDATER_PUBKEY` at build time (or edited into `tauri.conf.json`), so the owner never edits files; the repository keeps a placeholder.
- **Without secrets:** the run stays green, the app archive is a workflow artifact only, nothing is published.

## D-20 Word statistics: daily aggregates in the existing statistics database

- **Chosen:** migration 2 of `voxa_stats.db` adds `form_daily`, `ngram_daily`, `dictation_openers` and two roll-ups, `word_total` and `ngram_total` (see "Deviations"). One dictation is added in the same transaction that stores it (`VoxaStore::record`), through one long-lived connection, on a background thread. Nothing existing is altered.
- **Text handling:** the tokenizer of the basic statistics is reused; lowercase and `ё` -> `е` for the key; `rust-stemmers` (Russian for Cyrillic words, English for words with an ASCII letter); the shown word is the most frequent real form in the period. `Claude'а` and `Claude's` fold into `claude`, English contractions (`don't`) stay whole. Numbers are not words. Phrases (2 and 3 words) do not cross a sentence end or a number.
- **Deviations from the PLAN-2 B.3 table list, with the measurement behind them** (release build, synthetic database of 20 000 dictations of near-random text, which is the worst case for phrase counts):
  - `word_forms` and `word_daily` are not stored. `form_daily (stem, form, day)` has every form with its count per day and per-word sums are a `GROUP BY`; a dictation writes one row per word instead of three. The first version with all five tables took 26.7 ms per dictation; with day-first phrase keys, no secondary indexes and a warm connection 5.3 ms; with the cut tables 3.0 ms.
  - `ngram_daily` is keyed `(day, ngram)` (plan: `(ngram, day)`), so a dictation's phrases land in a few pages (0.5 ms for about 100 rows, against 2 ms for 46 word rows keyed by word).
  - `word_total` and `ngram_total` are roll-ups of the days up to `voxa_meta.words_rolled_through` (yesterday at the latest). All-time views read them plus the days after that date. Without them all-time queries scanned the whole table: 0.6 to 0.8 s for top words, phrases and vocabulary; with them 15 to 45 ms. Rolling happens at start and before a read, once a day; a dictation for an already rolled day is added to the totals directly and a delete subtracts from them, so they never disagree with the daily rows (tests compare every all-time answer before and after rolling).
- **Benchmark** (`cargo test --release words::tests::bench -- --ignored --nocapture`, 20 000 dictations over two years): indexing 3.0 ms per dictation (sandbox disk; it runs off the dictation path, so it adds no latency); every query for all time, 30 days and 7 days between 0.4 and 50 ms (limit 150 ms). The database file grows to a few hundred MB on that adversarial text; real speech repeats far more and the weekly cleanup (D-25) removes the single-use phrases.

## D-21 Stop words at query time, except for phrases

- **Chosen:** the database holds every word; the RU + EN lists (`words/stopwords.rs`) are applied when a list is built, by stem, so the switch "Show function words" recounts nothing. Phrases made only of stop words are flagged `stop_only` when written and left out of "frequent phrases".
- **Why for phrases:** the plan asks to leave those out without a switch, and filler phrases such as "как бы" consist of stop words, so they must still be counted for the filler block. The flag is derived data; a change of the list needs a recount.
- **Known limit:** a stem shared by a stop word and a content word is hidden together with it (for example "быт" from "быть").

## D-22 Periods are rolling windows

- **Chosen:** Week = the last 7 days, Month = the last 30 days, both ending today; All time; Custom = two dates. The UI computes local days, the backend only receives `from`/`to`.
- **Why:** it needs no locale rule for the first day of the week and matches "how did the last days go". The overview tab keeps its Monday-to-Sunday week.

## D-23 Deleting a dictation: explicit delete subtracts, cleanup does not

- **Chosen:** deleting an entry on the History screen (`HistoryManager::delete_entry`) also removes the dictation from the statistics: its row, and its words, phrases and opener are subtracted (counts that reach zero are deleted). Cleanup of old entries by count or age does not touch the statistics.
- **Why:** PLAN-2 B.3 asks for exactly this split. Subtracting needs the dictation to be indexed (it has an opener row); a row the backfill has not reached yet is only deleted, never subtracted.

## D-24 Backfill of older dictations

- **Chosen:** on the first start with the word tables, the dictations that exist at that moment are indexed once in the background (a thread, batches of 500, a 5 ms pause between batches). The target (highest id at that moment) and a cursor are kept in `voxa_meta`, and the cursor moves in the same transaction as the counts, so an interrupted run resumes and nothing is counted twice. Dictations recorded afterwards are indexed live and are above the target. Progress is exposed by `voxa_words_backfill_status` and the `voxa-words-backfill` event; the tab shows a banner with a progress bar.
- **Flag:** `voxa_meta.words_backfill_done` (the plan's `schema_meta` is the existing `voxa_meta`). "Clear statistics" empties the word tables and marks the backfill done.

## D-25 Phrase cleanup counts a phrase over all days

- **Chosen:** once a week, after rolling the days up, phrases that were seen exactly once in total (`ngram_total.count = 1`) and not for 90 days (`last_day`) are deleted from the totals and from `ngram_daily`.
- **Why not "count = 1 per day":** a phrase said once a day for months has `count = 1` on every row and would be deleted entirely. Only phrases with one use ever are dropped; phrases with two or more uses stay however old. The synthetic benchmark loses about 880 000 of its 1.66 million phrase rows this way.

## D-26 Filler words

- **Chosen:** the list lives in settings (`voxa.filler_words`, default: типо, типа, короче, ну, как бы, вот, значит, блин, э, ээ, эээ, м, мм), edited in Settings, Advanced. An entry of one word is counted by its form, of two or three words as a phrase. The trend always covers the last 12 weeks, longer when the period starts earlier (at most 52), and shows uses per 100 words.
- **Why 12 weeks regardless of the period:** a line of one or two points is no trend.

## D-27 Examples of a word

- **Chosen:** the five newest dictations of the period that contain the word are found by scanning up to 20 000 recent dictations, with a cheap first-letters check before the exact stem comparison; the matches are highlighted by byte spans.
- **Why:** SQLite's `LIKE` and `lower()` ignore case only for ASCII, so Cyrillic cannot be searched in SQL. No extra table is needed for a click-driven view.

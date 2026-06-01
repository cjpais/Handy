# Decision log

Record significant product and technical choices here so future-you (and agents) know **why**, not only **what**.

**Format for new entries:**

```markdown
## YYYY-MM-DD — Short title

**Status:** decided | proposed | superseded  
**Context:** …  
**Decision:** …  
**Consequences:** …  
**Alternatives considered:** …
```

---

## 2026-05-16 — Product fork, not rebrand

**Status:** decided  

**Context:** Forked [cjpais/Handy](https://github.com/cjpais/Handy) into [felixbaileymurray/goldfish](https://github.com/felixbaileymurray/goldfish). Goal is a new app with extended functionality, not rewriting offline STT from scratch.  

**Decision:** Treat Handy as an **engine** (audio, VAD, local ASR, paste, models). Build Goldfish as a **separate product** on top with new code in isolated `goldfish/` modules and minimal hooks in upstream-owned files.  

**Consequences:** No need for day-one find-replace of every “Handy” string. Need clear product boundary (bundle ID, updater, releases) before shipping to users.  

**Alternatives considered:** Full rebrand via global rename (rejected: merge pain, little value); rewrite STT stack (rejected: redundant with Handy).

---

## 2026-05-16 — Stay synced with upstream Handy

**Status:** decided  

**Context:** Want bugfixes and engine improvements from Handy without maintaining a divergent copy of core audio/transcription code.  

**Decision:** Add `upstream` remote pointing at `cjpais/Handy`; use a documented merge workflow (see [fork-strategy.md](./fork-strategy.md)). Prefer **two-branch** model (`upstream-sync` + `goldfish`) as Goldfish diverges. Maintain `UPSTREAM.md` at repo root (to be created) with last merged SHA.  

**Consequences:** Occasional merge conflicts in `tauri.conf.json`, `lib.rs`, `App.tsx`. Goldfish-only features stay in `src-tauri/src/goldfish/` and `src/goldfish/` to reduce conflict surface.  

**Alternatives considered:** Single `main` with direct upstream merges (acceptable early); vendoring core into a separate crate (deferred until merges are painful).

---

## 2026-05-16 — Separate app identity (bundle ID)

**Status:** decided / execution deferred  
**Trigger to execute:** Before the first build is distributed to a second machine, or before re-enabling the updater. Until then the dev build can keep `com.pais.handy` because no one is installing it elsewhere.

**Context:** Goldfish must be installable and identifiable as its own app, not an update channel for Handy.  

**Decision:** Use a new Tauri bundle identifier (e.g. `com.felixbaileymurray.goldfish`), `productName` “Goldfish”, own icons, and **disable or replace** Handy’s updater endpoint. The updater disable is being executed early (see 2026-05-19 entry) even though the rest of the identity split is deferred, because the updater is the highest-blast-radius footgun.

**Consequences:** When executed: new app data directory; users do not inherit Handy settings/models automatically; can run beside Handy.  

**Alternatives considered:** Keep `com.pais.handy` to reuse data dir (rejected for a distinct product).

---

## 2026-05-16 — Documentation in `docs/`

**Status:** decided  

**Context:** Need transparent, local documentation to track analysis and decisions over time.  

**Decision:** Keep project-specific docs in `docs/` (`codebase-overview.md`, `fork-strategy.md`, `decisions.md`). Leave upstream docs at repo root (`README.md`, `AGENTS.md`, `BUILD.md`).  

**Consequences:** Update this file when making non-obvious choices; refresh overview when architecture changes materially after upstream merges.

---

## 2026-05-19 — Single `main` branch for now, two-branch when triggered

**Status:** decided  

**Context:** [fork-strategy.md](./fork-strategy.md) originally recommended a two-branch model (`upstream-sync` + `goldfish`). On review, the value of that split is staging upstream merges and enabling clean cherry-picks back upstream — neither matters yet (no users, no releases, no plan to contribute fixes upstream). The branch model does not help with separating "what's ours vs. theirs"; directory layout does that.

**Decision:** Stay on a single `main` branch, merging from `upstream/main` directly. Identify Goldfish-only code by path (`src-tauri/src/goldfish/`, `src/goldfish/`).

**Triggers to switch to two-branch:**

1. An upstream merge breaks something and we need to ship a Goldfish-only hotfix without pulling in the rest of that merge.
2. We start contributing engine fixes back upstream and need a clean branch to cherry-pick from.

**Consequences:** Simpler day-to-day workflow. The cost of switching later is mechanical: create `upstream-sync` from the current `upstream/main` baseline, rename `main` → `goldfish`. No history rewrite needed.

**Alternatives considered:** Two-branch from day one (rejected: pure overhead at this stage).

---

## 2026-05-19 — `upstream` remote wired up

**Status:** decided / executed

**Context:** Fork strategy assumed upstream syncs were happening; they weren't. Only `origin` was configured.

**Decision:** Added `upstream` remote pointing at `cjpais/Handy.git`, fetched it, created [UPSTREAM.md](../UPSTREAM.md) with merge workflow + merge log. Baseline: `e3206aa` (Goldfish `main` is at upstream HEAD plus 3 local commits — fork docs and macOS build fixes).

**Consequences:** Future merges have a recorded baseline. UPSTREAM.md is the canonical workflow doc; fork-strategy.md is the rationale doc.

---

## 2026-05-19 — Defer post-transcription hook location

**Status:** decided  

**Context:** The original fork-strategy.md named `process_transcription_output` as the canonical post-transcription hook. That function does not exist in the codebase — it was invented by an earlier doc-writing pass. The real pipeline runs through `transcription_coordinator.rs::stop()` and `managers/transcription.rs`.

**Decision:** Do not pre-commit to a hook location. When the first feature needs to react to transcription output, pick the hook then — with the actual threading and lifecycle of that feature in mind.

**Consequences:** Slightly more thinking required per feature; far less risk of building on a wrong abstraction.

**Alternatives considered:** Carve out a generic "post-transcription" event/listener up front (rejected as speculative generality).

---

<!-- Add new decisions below this line -->

## 2026-05-30 — Settings as a full-panel overlay, not a sidebar section

**Status:** decided  
**Context:** Goldfish needs to grow beyond a settings panel into a real product with distinct functional areas. The existing pattern — every section driven by `currentSection` state — would force settings to compete for sidebar real estate with first-class product views.  
**Decision:** Replace `currentSection` string state with a `view: "main" | "settings"` toggle. Settings become a full-panel overlay entered via a gear icon and exited via a back button; the main panel is free to host non-settings content.  
**Consequences:** Settings are clearly secondary to product views. Adding new top-level product areas requires no restructuring of the settings panel.  
**Alternatives considered:** Keep settings as a sidebar section alongside future product sections (rejected: clutters primary navigation and implies settings is a peer of product areas).

---

## 2026-05-30 — Sidebar scoped to product areas only

**Status:** decided  
**Context:** With settings moved to an overlay, the sidebar's role needed redefining.  
**Decision:** Sidebar shows only top-level product areas. Settings, model status, update checker, and version info are not sidebar items — settings is a gear button, status info was in the retired Footer.  
**Consequences:** Sidebar stays clean as the product grows. No settings-related cruft in primary navigation.  
**Alternatives considered:** Sidebar containing both product areas and a settings link at the bottom (rejected: mixes navigation hierarchy).

---

## 2026-05-30 — Footer retired, then immediately reinstated on floral layer

**Status:** decided (initial retirement superseded same session)  
**Context:** Footer initially retired to simplify the layout restructure. Model load status, update checker, and version had no natural home. Rather than deferring indefinitely, the decision was reversed the same session.  
**Decision:** Reinstate Footer on the floral window background — outside and below the white inner panel — rather than inside the main layout. Model load status bottom-left; update checker and version bottom-right.  
**Consequences:** Status chrome is visually separated from product chrome by material (floral vs white). Future UI layers respect this two-tier hierarchy: product content inside the white panel, utility/status on the floral background.  
**Alternatives considered:** Keep Footer retired and defer status display (rejected: the information is useful immediately); keep Footer inside the white panel as before (rejected: mixes navigation tier with status tier).

---

## 2026-05-30 — UpdateChecker moved to About settings; Footer reduced to ModelSelector only

**Status:** decided  
**Context:** Footer contained ModelSelector, UpdateChecker, and version string — too much chrome at one visual level with no clear grouping rationale.  
**Decision:** Remove UpdateChecker and version string from Footer. Surface UpdateChecker in About settings inline with the version row. Footer shows only ModelSelector, right-aligned.  
**Consequences:** Footer is minimal and unambiguous. Update availability is visible only in Settings → About, acceptable in dev phase. Revisit when Goldfish updater is wired for non-dev users.  
**Alternatives considered:** Keep UpdateChecker in Footer alongside ModelSelector (rejected: groups unrelated controls, visually cluttered); move ModelSelector into Sidebar (rejected: sidebar has a clear role as product navigation).

---

## 2026-05-30 — Capture area maps to HistorySettings as a placeholder

**Status:** decided  
**Context:** The main panel needed non-settings content to justify the view split, but building a real Capture UI was out of scope.  
**Decision:** Main view renders `HistorySettings` as a stand-in for the Entries area. This is explicitly temporary — real Entries UI will replace it.  
**Trigger to revisit:** When building purpose-built Entries / Capture UI.  
**Consequences:** The structural separation is in place without blocking on UI design. `HistorySettings` is rendered in a context it wasn't designed for.  
**Alternatives considered:** Leave main panel empty (rejected: confusing); build minimal Capture UI now (rejected: out of scope).

---

## 2026-05-30 — "Capture" renamed to "Entries" in the sidebar

**Status:** decided  
**Context:** The sidebar previously had a "Capture" section framing STT + post-processing as a peer product area. Recording and transcription are the engine, not a destination users navigate to.  
**Decision:** Rename "Capture" to "Entries". The primary thing a user sees is the output of a recording session, not the act of capturing. The capture mechanism is ambient.  
**Consequences:** IA reflects the user's actual goal (reviewing outputs) rather than the app's internal mechanism. Future product areas fit naturally alongside Entries.  
**Alternatives considered:** Keep "Capture" (rejected: misrepresents what users are doing); create a separate "Recordings" section (rejected: unnecessary split at this stage).

---

## 2026-05-30 — Entry card hierarchy: title → metadata → output → details accordion

**Status:** decided  
**Context:** The existing history entry layout showed raw transcript or post-processed text without hierarchy. The user's mental model is: see the summary first, dig into raw data only if needed.  
**Decision:** Each entry card renders: (1) derived title (first sentence of post-processed or raw transcript, capped at 72 chars); (2) metadata (formatted date/time, muted); (3) main output (post_processed_text preferred, raw transcript fallback); (4) collapsible details accordion (raw transcript when summary exists + audio player).  
**Consequences:** Surface view is always the highest-value output. Raw audio and transcript are accessible but not prominent. Layout reinforces that summary is the product; transcript and audio are evidence.  
**Alternatives considered:** Show transcript as primary with summary below (rejected: inverts the value hierarchy); hide audio entirely (rejected: user may need to interrogate the source recording).

---

## 2026-05-30 — Entry card title derivation is a placeholder pending backend summarisation

**Status:** proposed  
**Context:** Current title is derived client-side from the first sentence of `post_processed_text` or `transcription_text`. A dedicated summarisation pipeline will produce structured data (proper title, summary, action items).  
**Decision:** Keep first-sentence derivation as a stand-in. Do not invest in making it smarter. When backend summarisation lands, redesign the card against the actual data shape.  
**Trigger to revisit:** When backend summarisation pipeline produces a concrete output schema.  
**Consequences:** Card may look rough for entries with no post-processed text, but avoids building against an unstable data contract.  
**Alternatives considered:** More sophisticated client-side title extraction (rejected: thrown away once backend pipeline lands).

---

## 2026-05-30 — Summarisation shares provider + API key with post-processing; model and prompt are independent

**Status:** decided  
**Context:** Summarisation needs an LLM provider and API key. Options were: (a) fully independent, (b) shared provider + key with independent model + prompt, (c) fully shared. Fully independent duplicates UI and key storage; fully shared prevents independent model tuning.  
**Decision:** Summarisation inherits provider and API key from post-processing via a single-chokepoint helper. Only model selection and prompt are independently configurable. The Summarisation settings panel shows provider as read-only (inherited) to make the dependency explicit.  
**Trigger to revisit:** If user wants to use a different provider for summarisation than for post-processing — the settings struct is shaped to make this additive.  
**Consequences:** Single API key entry; users cannot mix providers across features today.  
**Alternatives considered:** Fully independent provider + key (rejected: duplicate entry for no MVP benefit); fully shared including model (rejected: prevents independent model/prompt tuning).

---

## 2026-05-30 — Background summarisation auto-triggered after pipeline save

**Status:** decided  
**Context:** Summarisation could be triggered manually, automatically in the background, or both. Running inline before paste would block the user's flow on an LLM call.  
**Decision:** Summarisation fires automatically as a detached background task immediately after `save_entry` — if `summarize_enabled` is true and the entry has content. Entry's `summary_status` column tracks pending/done/error. Manual re-trigger via `summarize_history_entry` is also available for retries.  
**Consequences:** Users get instant paste without waiting for summary. Failures are silent unless the UI surfaces `summary_status`. Background panics do not affect paste flow.  
**Alternatives considered:** Manual-only trigger (rejected: too much friction for ambient capture use case); blocking summarisation before paste (rejected: adds latency to every recording).

---

## 2026-05-30 — Summarisation uses structured JSON schema output

**Status:** decided  
**Context:** Summarisation needs both a title and action items — two distinct fields rather than a prose blob. The existing LLM client already exposes `send_chat_completion_with_schema(...)`.  
**Decision:** Summarisation calls `send_chat_completion_with_schema` with a schema producing `{ title: string, actions: [{ text, completed }] }`. Reuses existing infrastructure and produces machine-readable output the UI can render as a checklist.  
**Consequences:** Only works with providers/models supporting structured/JSON-mode output. Providers that don't support JSON schema constraints cannot be used for summarisation even if they work for post-processing.  
**Alternatives considered:** Prompt-only output with client-side parsing (rejected: fragile); separate API calls for title vs actions (rejected: unnecessary latency and complexity).

---

## 2026-05-30 — Summarisation gets a dedicated section in settings

**Status:** decided  
**Context:** Summarisation-specific settings (model, prompt selection) were co-located with or adjacent to post-processing settings. As summarisation grows, its own panel makes the boundary explicit and leaves room for future API key fields.  
**Decision:** Add a "Summarisation" section to the settings panel, parallel to the existing Post-processing section. The panel holds model picker and prompt selector; the enable/disable toggle lives outside it (in Advanced, mirroring the post-process toggle pattern) to avoid a chicken-and-egg situation where the section is gated on the toggle it contains.  
**Consequences:** Clear settings boundary between post-processing and summarisation. Navigation structure anticipates eventual provider independence without requiring it now.  
**Alternatives considered:** Fold model + prompt into Advanced settings alongside the toggle (rejected: Advanced would grow cluttered); put the toggle inside the Summarisation section (rejected: user cannot reach configuration if the section is hidden until enabled).

---

## 2026-05-30 — Post-processing and Summarisation promoted out of Experimental

**Status:** decided  
**Context:** Both toggles previously lived inside the "Experimental Features" gated group in Advanced settings, implying alpha/unstable status and hiding both features behind an extra toggle.  
**Decision:** Move both toggles into a new always-visible "Processing" group in Advanced settings. Neither is gated. Toggles still default to off. Experimental Features group retained for keyboard impl, acceleration, and lazy-stream-close.  
**Consequences:** Users can enable/disable post-processing and summarisation without discovering the Experimental toggle first. Post-processing parity with Handy upstream must be watched — when Handy adds its own post-processing, there may be merge conflicts.  
**Alternatives considered:** Leave both in Experimental until a formal QA pass (rejected: functionality is stable enough; the gate was causing unnecessary friction).

---

## 2026-06-01 — Two-pipeline product mental model

**Status:** decided  
**Context:** Goldfish grew beyond Handy's single-loop model with the addition of summarisation. Two distinct output pipelines now exist sharing a common capture/transcription stage but diverging at the point of use.  
**Decision:** The canonical mental model is: **Shared:** Capture → Transcribe → Post-process. **Pipeline A (immediate):** → Paste. **Pipeline B (deferred):** → Store → Review → Summarise. Post-processing is a shared enrichment step, not a paste-specific concern. The IA of settings, documentation, and future feature placement should reflect this split.  
**Consequences:** Post-processing belongs neither in Output nor in Summarisation — it sits in the shared pipeline. Any future pipeline stage is evaluated against where it fits in this model, not which UI section it resembles.  
**Alternatives considered:** Post-processing as an Output concern (rejected: it produces enriched text consumed by both pipelines); treating each pipeline as fully independent with duplicated settings (rejected: unnecessary complexity for shared config like provider/key).

---

## 2026-06-01 — Settings IA restructured to 9 pipeline-order sections

**Status:** decided  
**Context:** Handy's settings were structured around a single feature so grouping by UI concern was fine. Goldfish has multiple pipeline stages with independent configuration. The previous grouping (General, Models, Advanced, conditional Post-processing/Summarisation) bundled shortcuts into feature sections and mixed pipeline-stage concerns with app-level concerns.  
**Decision:** Replace the previous section layout with 9 sections in pipeline order: **Shortcuts** (cross-cutting setup), **Capture** (mic, audio, clamshell, history retention), **Transcription** (model library, language, custom words — formerly "Models"), **Post-processing** (shared enrichment stage), **Output** (paste behaviour, clipboard), **Summarisation** (model, prompt, enable), **App** (appearance, language, tray, updates), **About** (version, credits), **Debug** (dev-only). Post-processing and Summarisation are always visible with their toggles inline rather than conditionally appearing when enabled.  
**Consequences:** Adding a new pipeline feature has a natural home without restructuring. "Models" disappears as a section name. Shortcuts are not duplicated across sections. The order communicates the product's flow to users.  
**Alternatives considered:** Keep per-feature grouping with Post-processing and Summarisation as siblings in an AI section (rejected: conflates two pipeline stages with different timing and config); cross-cutting concern grouping (rejected: hides pipeline order which is the user's mental model).

---

## 2026-06-01 — Shortcuts extracted as a first-class cross-cutting setup section

**Status:** decided  
**Context:** Keyboard shortcuts were previously embedded inside their respective feature sections. As Goldfish adds features, each section would independently accumulate shortcut settings — creating a fragmented setup experience.  
**Decision:** All shortcuts (transcribe, cancel, push-to-talk, post-process hotkey) live in a single **Shortcuts** section at the top of settings navigation. No other section contains shortcut settings. This section is conceptually a "setup stage" — users visit it once during configuration.  
**Consequences:** Future features that need a hotkey add their shortcut to the Shortcuts section only. A future onboarding flow can target this single section for first-run shortcut setup without scraping settings from multiple panels.  
**Alternatives considered:** Keep shortcuts in their respective feature sections (rejected: fragments setup experience); shortcuts as a subsection of App settings (rejected: underweights setup importance; users configure shortcuts before configuring appearance).

---

## 2026-06-01 — Onboarding flow for shortcut setup deferred to backlog

**Status:** proposed  
**Context:** Extracting shortcuts into a dedicated section creates a clean target for a first-run onboarding flow. Goldfish requires keyboard shortcuts to operate — a user who never discovers or sets them cannot record anything.  
**Decision:** Log as a named backlog item (Notion: "Onboarding flow: keyboard shortcuts setup", P2, Feature + UX/UI). Deferred. When built, the flow should trigger on first app launch and guide users through setting the transcribe shortcut as a minimum.  
**Trigger to revisit:** When Goldfish moves toward a first public release or beta; or when user research shows shortcut discovery is a friction point.  
**Consequences:** New users must discover shortcuts independently via settings until the flow exists. The Shortcuts section being first in the nav is a mitigation.  
**Alternatives considered:** Build minimal onboarding immediately (rejected: out of scope for the IA restructure); add a persistent first-run banner (rejected: deferred with the full onboarding scope rather than half-implementing a hint system).

---

## 2026-06-01 — `.claude/commands/` excluded from version control

**Status:** decided  
**Context:** A personal slash command was accidentally committed containing a personal Notion database URL. The repo is a public fork intended to be forkable by others; personal workflow configuration has no value to external contributors.  
**Decision:** Add `.claude/commands/` to `.gitignore`. Personal slash commands live only in the local working tree. `.claude/settings.json` and hook definitions remain tracked because they describe project-level behaviour useful to contributors.  
**Consequences:** Future personal commands are invisible to git by default. Contributors who fork the repo will not inherit personal workflow configuration.  
**Alternatives considered:** Remove in a follow-up commit (rejected: URL would remain visible in public history); make the repo private (rejected: not possible for a GitHub fork without duplicating); scrub personal URLs from the command file (rejected: these are personal workflows, not project infrastructure).

---

<!-- Session b1ae6ff7 — 2026-05-30 12:55 — review, edit, then move entries to decisions.md -->

## 2026-05-30 — Settings as a full-panel overlay, not a sidebar section

**Status:** decided  
**Context:** Goldfish needs to grow beyond a settings panel into a real product with distinct functional areas (Capture, Summarisation, Connections). The existing pattern — every section a settings page driven by `currentSection` state — would force settings to compete for sidebar real estate with first-class product views.  
**Decision:** Replace `currentSection` string state with a `view: "main" | "settings"` toggle. Settings become a full-panel overlay entered via a gear icon and exited via a back button; the main panel is free to host non-settings content.  
**Consequences:** Settings are clearly secondary to product views. Adding new top-level product areas (Summarisation, Connections) requires no restructuring of the settings panel. Users navigate settings as a mode, not a destination.  
**Alternatives considered:** Keep settings as a sidebar section alongside future product sections (rejected: clutters primary navigation and implies settings is a peer of Capture/Summarise).

---

## 2026-05-30 — Sidebar scoped to product areas only (not settings links)

**Status:** decided  
**Context:** With settings moved to an overlay, the sidebar's role needed to be redefined.  
**Decision:** Sidebar shows only top-level product areas (Capture for now; Summarisation and Connections as future stubs). Settings, model status, update checker, and version info are not sidebar items — settings is a gear button, status info was in the retired Footer.  
**Consequences:** Sidebar stays clean as the product grows. No settings-related cruft in primary navigation. Footer component retired entirely.  
**Alternatives considered:** Sidebar containing both product areas and a settings link at the bottom (rejected: mixes navigation hierarchy).

---

## 2026-05-30 — Footer retired; status info deferred

**Status:** decided  
**Context:** Footer previously held model-loader status, update-checker, and version display. Under the new layout these had no natural home and were not critical to the initial restructure.  
**Decision:** Delete Footer entirely. Model/version/update status display is deferred — to be re-homed when a specific product need surfaces (e.g. persistent status bar, settings About section, or overlay HUD).  
**Consequences:** Some useful status info temporarily invisible in the UI. Technical debt is low because the underlying components (ModelDropdown, UpdateChecker) are untouched; only the container was removed.  
**Alternatives considered:** Move Footer content into the sidebar footer slot (rejected: layout not designed for it yet); move into settings About section immediately (rejected: scope creep for this task).

---

## 2026-05-30 — Capture area maps directly to existing HistorySettings for now

**Status:** decided  
**Context:** The main panel needed non-settings content to justify the view split, but building a real Capture UI was out of scope.  
**Decision:** The main view renders `HistorySettings` as a placeholder for the Capture area. This is explicitly a stand-in — real Capture UI (live recording state, transcript list, etc.) will replace it.  
**Trigger to revisit:** When Summarisation & Actions feature work begins, the Capture view needs to be purpose-built rather than reused settings component.  
**Consequences:** The structural separation is in place without blocking on Capture UI design. `HistorySettings` is rendered in a context it wasn't designed for.  
**Alternatives considered:** Leave main panel empty until Capture UI is designed (rejected: empty panel is confusing); build minimal Capture UI now (rejected: out of scope for this structural pass).

---

<!-- Session b1ae6ff7 — 2026-05-30 19:02 — review, edit, then move entries to decisions.md -->

## 2026-05-30 — Footer retirement reversed; status info relocated to floral window layer

**Status:** decided (supersedes "Footer retired; status info deferred")  
**Context:** After the initial restructure retired the Footer, the status information (model load, update checker, version) had no home. Rather than deferring it entirely, the decision was made to restore it immediately but in a different location.  
**Decision:** Reinstate the Footer component, but render it on the floral window background — outside and below the white inner panel — rather than inside the main layout. Model load status sits bottom-left; update checker and version sit bottom-right. The settings gear remains at the bottom of the sidebar, flush with the white inner panel's bottom edge.  
**Consequences:** Status chrome is visually separated from product chrome by material (floral vs white). The Footer component is not truly retired — only its position changed. Future UI layers need to respect this two-tier hierarchy: product content inside the white panel, utility/status on the floral background.  
**Alternatives considered:** Keep Footer retired and defer status display (rejected: the information is useful immediately); move status into settings About section (rejected: hides active state behind a modal); keep Footer inside the white inner panel as before (rejected: mixes navigation tier with status tier).

---

<!-- Session 5e285567 — 2026-05-30 19:11 — review, edit, then move entries to decisions.md -->

## 2026-05-30 — UpdateChecker moved to About settings; Footer reduced to ModelSelector only

**Status:** decided  
**Context:** With the Footer reinstated as a utility layer on the floral background, it contained three elements: ModelSelector, UpdateChecker, and version string. Having both the gear/settings button at the bottom of the sidebar and ModelSelector + UpdateChecker in the Footer created visual awkwardness — too much chrome at the same visual level with no clear grouping rationale.  
**Decision:** Remove UpdateChecker and version string from Footer entirely. Surface UpdateChecker in the About section of settings, inline with the version row. Footer now shows only ModelSelector, right-aligned. The update control is deliberately deprioritised — it will not be used during active development and does not need to be prominent.  
**Consequences:** Footer is minimal and unambiguous (one control, one purpose). Update availability is only visible if the user opens Settings → About, which is acceptable given the dev-phase context. When the updater is eventually wired up for Goldfish releases, this placement may need revisiting.  
**Trigger to revisit:** When Goldfish updater endpoint is configured and the app ships to non-dev users.  
**Alternatives considered:** Keep UpdateChecker in Footer alongside ModelSelector (rejected: groups unrelated controls, visually cluttered); move ModelSelector into Sidebar above the settings gear (rejected: sidebar already has a clear role as product navigation).

---

<!-- Session 2972a180 — 2026-05-30 19:27 — review, edit, then move entries to decisions.md -->

## 2026-05-30 — Capture is engine-level, not a top-level product section

**Status:** decided  
**Context:** The sidebar previously had a "Capture" section that framed STT + post-processing as a peer product area. In reality, recording and transcription are the engine beneath the product — not a destination users navigate to.  
**Decision:** Rename "Capture" to "Entries" in the sidebar. The primary thing a user sees is the output of a recording session (summary, metadata, transcript) — not the act of capturing. The capture mechanism itself is ambient.  
**Consequences:** Product information architecture now reflects the user's actual goal (reviewing outputs) rather than the app's internal mechanism (capturing audio). Future product areas (Summarisation, Connections) fit naturally alongside Entries without implying a parallel "recording mode."  
**Alternatives considered:** Keep "Capture" as the section name (rejected: misrepresents what users are doing in that view); create a separate "Recordings" section distinct from "Capture" (rejected: unnecessary split at this stage).

---

## 2026-05-30 — Entry card hierarchy: title → metadata → output → details accordion

**Status:** decided  
**Context:** The existing history entry layout showed raw transcript or post-processed text without hierarchy. The user's mental model is: see the summary first, dig into raw data only if needed.  
**Decision:** Each entry card renders in this order: (1) derived title (first sentence of post-processed or raw transcript, capped at 72 chars, falling back to timestamp); (2) metadata (formatted date/time, muted); (3) main output (post_processed_text preferred, raw transcript fallback); (4) collapsible details accordion (raw transcript when summary exists + audio player).  
**Consequences:** The surface view is always the highest-value output. Raw audio and transcript are accessible but not prominent. Layout reinforces that summary is the product; transcript and audio are evidence.  
**Alternatives considered:** Show transcript as primary with summary below (rejected: inverts the value hierarchy); hide audio entirely (rejected: user may need to interrogate the source recording).

---

<!-- Session 2972a180 — 2026-05-30 19:30 — review, edit, then move entries to decisions.md -->

## 2026-05-30 — Capture is engine-level, not a top-level product section

**Status:** decided  
**Context:** The sidebar previously had a "Capture" section that framed STT + post-processing as a peer product area. In reality, recording and transcription are the engine beneath the product — not a destination users navigate to.  
**Decision:** Rename "Capture" to "Entries" in the sidebar. The primary thing a user sees is the output of a recording session (summary, metadata, transcript) — not the act of capturing. The capture mechanism itself is ambient.  
**Consequences:** Product information architecture now reflects the user's actual goal (reviewing outputs) rather than the app's internal mechanism (capturing audio). Future product areas (Summarisation, Connections) fit naturally alongside Entries without implying a parallel "recording mode."  
**Alternatives considered:** Keep "Capture" as the section name (rejected: misrepresents what users are doing in that view); create a separate "Recordings" section distinct from "Capture" (rejected: unnecessary split at this stage).

---

## 2026-05-30 — Entry card hierarchy: title → metadata → output → details accordion

**Status:** decided  
**Context:** The existing history entry layout showed raw transcript or post-processed text without hierarchy. The user's mental model is: see the summary first, dig into raw data only if needed.  
**Decision:** Each entry card renders in this order: (1) derived title (first sentence of post-processed or raw transcript, capped at 72 chars, falling back to timestamp); (2) metadata (formatted date/time, muted); (3) main output (post_processed_text preferred, raw transcript fallback); (4) collapsible details accordion (raw transcript when summary exists + audio player).  
**Consequences:** The surface view is always the highest-value output. Raw audio and transcript are accessible but not prominent. Layout reinforces that summary is the product; transcript and audio are evidence.  
**Alternatives considered:** Show transcript as primary with summary below (rejected: inverts the value hierarchy); hide audio entirely (rejected: user may need to interrogate the source recording).

---

## 2026-05-30 — Entry card title and hierarchy will change when backend summarisation lands

**Status:** proposed  
**Context:** Current entry card title is derived client-side from the first sentence of `post_processed_text` or `transcription_text`. This is a placeholder — the user intends to add a dedicated summarisation pipeline that produces structured data (proper title, summary, etc.) beyond what Whisper post-processing provides.  
**Decision:** Keep the first-sentence derivation as a stand-in. Do not invest in making it smarter. When the backend summarisation pipeline is ready, the user will supply an example data shape to drive a purpose-built redesign of the entry card.  
**Trigger to revisit:** User provides a concrete example of the summarisation output data shape and pipeline design.  
**Consequences:** The current layout may look slightly rough for entries with no post-processed text, but avoids building against an unstable data contract.  
**Alternatives considered:** Build a more sophisticated client-side title extraction now (rejected: would be thrown away once the backend pipeline lands).

---

<!-- Session 95b26534 — 2026-05-30 20:31 — review, edit, then move entries to decisions.md -->

## 2026-05-30 — Summarisation shares provider + API key with post-processing; model and prompt are independent

**Status:** decided  
**Context:** Summarisation needs an LLM provider, model, and API key. The options were: (a) fully shared with post-processing, (b) fully independent, or (c) shared provider + key with independent model + prompt. Fully independent would require a duplicate provider catalog and separate key entry UI. Fully shared would prevent using a different model for summarisation.  
**Decision:** Summarisation inherits provider and API key from post-processing settings via a single-chokepoint helper. Only model and prompt are independently configurable. This works because API keys are stored per-provider — sharing the key necessarily means sharing the provider.  
**Consequences:** Users do not re-enter credentials. Summarisation and post-processing cannot use different providers without a future settings split. The shared chokepoint is explicit in code, making the split mechanical when needed.  
**Trigger to revisit:** User wants summarisation to run against a different provider (e.g. a cheaper model from a different vendor) than post-processing.  
**Alternatives considered:** Fully independent provider + key + model (rejected: duplicate UI and key storage for no immediate benefit); fully shared including model (rejected: no way to tune the summarisation model independently).

---

## 2026-05-30 — Summarisation runs as an automatic background task after save, not inline

**Status:** decided  
**Context:** The existing pipeline runs Chinese conversion → post-process → save → paste, all inline before returning. Adding summarisation inline would block the paste (and therefore the user's flow) on an LLM call.  
**Decision:** Summarisation is triggered automatically in the background after `save_entry` completes. The paste is not delayed. The entry is written to the DB with a `summary_status` field that tracks pending/complete/error, so the UI can reflect progress asynchronously.  
**Consequences:** Users get instant paste without waiting for the summary. The entry card must handle a "summary pending" state. Background panics or LLM errors do not surface to the user via the paste flow — they are visible only via `summary_status`.  
**Alternatives considered:** Run summarisation inline before paste (rejected: LLM latency would delay the paste); run summarisation on user demand only (rejected: user confirmed auto-trigger is preferred).

---

## 2026-05-30 — Summarisation uses structured JSON schema output (title + action items)

**Status:** decided  
**Context:** The existing LLM client already exposes `send_chat_completion_with_schema(...)` for structured (JSON schema-constrained) output. Summarisation needs both a title and a list of action items — two distinct fields rather than a prose blob.  
**Decision:** Summarisation calls `send_chat_completion_with_schema` with a schema that produces `{ title: string, actions: [{ text, completed }] }`. This reuses existing infrastructure and produces machine-readable output the UI can render as a checklist without further parsing.  
**Consequences:** Summarisation only works with providers and models that support structured/JSON-mode output. Providers that do not support JSON schema constraints (e.g. some local models) cannot be used for summarisation even if they work for post-processing.  
**Alternatives considered:** Prompt-only output with client-side parsing (rejected: fragile; summary and actions would need regex or heuristic extraction); separate API calls for title vs. actions (rejected: unnecessary latency and complexity).

---

## 2026-05-30 — Summarisation settings get a dedicated panel in the settings submenu

**Status:** proposed  
**Context:** Summarisation-specific settings (model, prompt selection) are currently co-located with or adjacent to post-processing settings. As summarisation grows (independent provider selection, multiple prompt templates), having its own panel makes the boundary explicit and leaves room for future API key fields.  
**Decision:** Add a "Summarisation" section to the settings panel, parallel to the existing Post-processing section. The panel holds model picker and prompt selector now; provider and key fields are stubbed or omitted until the shared-key constraint is lifted.  
**Consequences:** Clear settings boundary between post-processing and summarisation. Navigation structure anticipates the eventual provider-independence split without requiring it now.  
**Alternatives considered:** Keep summarisation settings inside the Post-processing panel (rejected: conflates two pipeline stages that will eventually have independent configuration).

---

<!-- Session 95b26534 — 2026-05-30 20:31 — review, edit, then move entries to decisions.md -->

## 2026-05-30 — Summarisation shares provider + API key with post-processing

**Status:** decided  
**Context:** Summarisation needs an LLM provider and API key. The options were (a) fully independent provider + key + model + prompt, (b) shared provider + key with independent model + prompt, or (c) fully shared (identical config). The simplest MVP was considered, but fully sharing everything would prevent independent model/prompt tuning.  
**Decision:** Summarisation inherits provider and API key from post-processing via a single-chokepoint helper in settings.rs. Only model selection and prompt are independently configurable per feature. This avoids duplicate key-entry UX while keeping the summarisation prompt/model separate.  
**Trigger to revisit:** If the user wants to use a different provider (e.g. Anthropic for summarisation, OpenAI for post-processing), split provider + key into per-feature fields at that point — the settings struct is already structured so this is additive.  
**Consequences:** Single API key entry; users cannot mix providers across features today. The Summarisation settings panel shows provider as read-only (inherited) to make the dependency explicit. Settings struct is shaped so independent provider/key fields can be added without restructuring.  
**Alternatives considered:** Fully independent provider + key per feature (rejected: duplicate entry, more complex UI for no MVP benefit); fully shared everything (rejected: prevents independent model/prompt tuning).

---

## 2026-05-30 — Background summarisation auto-triggered after pipeline save

**Status:** decided  
**Context:** Summarisation could be triggered manually (user taps a button on an entry), automatically in the background after transcription completes, or both.  
**Decision:** Summarisation fires automatically as a detached background task immediately after `save_entry` in the transcription pipeline — if `summarize_enabled` is true and the entry has content. No user action required. The entry's `summary_status` column tracks in-progress / done / error state.  
**Consequences:** Users get summaries without friction. Failures are silent unless the UI surfaces `summary_status`. The pipeline is fire-and-forget; a summarisation failure does not block paste or history saving. Manual re-trigger via `summarize_history_entry` command is also available for retries.  
**Alternatives considered:** Manual-only trigger (rejected: too much friction for ambient capture use case); blocking summarisation before paste (rejected: adds latency to every recording even when summarisation is slow or fails).

---

## 2026-05-30 — Dedicated Summarisation section in settings panel

**Status:** decided  
**Context:** The summarisation feature needs configurable model and prompt. The options were to fold these into Advanced settings (alongside the enable toggle) or create a standalone section.  
**Decision:** Summarisation gets its own section in the settings panel (`SummarisationSettings`, registered under the `summarisation` key with a FileText icon). It shows model dropdown, prompt editor, and a read-only inherited-provider note. The enable/disable toggle lives in Advanced settings (mirroring the post-process toggle pattern) to avoid a chicken-and-egg situation where the section is gated on the toggle it contains.  
**Consequences:** Clear settings information architecture: Advanced holds feature switches; dedicated sections hold per-feature configuration. When provider/key independence is added, the Summarisation section gains those fields naturally.  
**Alternatives considered:** Fold model + prompt into Advanced settings alongside the toggle (rejected: Advanced would grow cluttered as features multiply); put the toggle inside the Summarisation section (rejected: user cannot reach configuration if the section is hidden until enabled).

---

<!-- Session 95b26534 — 2026-05-30 21:41 — review, edit, then move entries to decisions.md -->

## 2026-05-30 — Post-processing and Summarisation promoted out of Experimental

**Status:** decided  
**Context:** Both the Post-processing toggle and the Summarisation toggle previously lived inside the "Experimental Features" gated group in Advanced settings. This implied alpha/unstable status and hid both features behind an extra toggle.  
**Decision:** Move both toggles into a new always-visible "Processing" group in Advanced settings. Neither is gated — they are visible and configurable without enabling Experimental Features.  
**Consequences:** Users can enable/disable post-processing and summarisation without first discovering the Experimental Features toggle. Post-processing parity with Handy upstream must be tracked carefully — when Handy adds its own post-processing feature, there may be merge conflicts or a need to reconcile implementations. Experimental Features group still exists for keyboard impl, acceleration, and lazy-stream-close.  
**Alternatives considered:** Leave both in Experimental until a formal QA pass (rejected: functionality is stable enough for use; the gate was causing unnecessary friction).

---

<!-- Session 95b26534 — 2026-05-30 21:41 — review, edit, then move entries to decisions.md -->

## 2026-05-30 — Post-processing and Summarisation promoted out of Experimental

**Status:** decided  
**Context:** Both the Post-processing toggle and the Summarisation toggle previously lived inside the "Experimental Features" gated group in Advanced settings. This implied alpha/unstable status and hid both features behind an extra toggle. Post-processing is a step change from upstream Handy, so the promotion warrants watching the upstream remote for conflicts.  
**Decision:** Move both toggles into a new always-visible "Processing" group in Advanced settings. Neither is gated — they are visible and configurable without enabling Experimental Features. The toggles still default to off, so features don't activate until a user opts in.  
**Consequences:** Users can enable/disable post-processing and summarisation without first discovering the Experimental Features toggle. Post-processing parity with Handy upstream must be tracked carefully — when Handy adds its own post-processing feature, there may be merge conflicts or a need to reconcile implementations. Experimental Features group still exists for keyboard impl, acceleration, and lazy-stream-close.  
**Alternatives considered:** Leave both in Experimental until a formal QA pass (rejected: functionality is stable enough for use; the gate was causing unnecessary friction).

---

<!-- Session 1d2dd058 — 2026-06-01 12:40 — review, edit, then move entries to decisions.md -->

## 2026-06-01 — `settings_store.json` excluded from version control

**Status:** decided  
**Context:** Goldfish stores user API keys at runtime in a `settings_store.json` file. Because the repo is a public fork that cannot be made private, any accidental commit of that file would expose real credentials publicly.  
**Decision:** Add `settings_store.json` to `.gitignore` (at all tree levels) so it can never be accidentally committed, even when using Tauri's portable store mode.  
**Consequences:** Real API keys remain outside the repo. The ignore rule is in place before any key was ever committed, so no history scrub is needed. Developers who clone the repo will need to re-enter their own keys on first run.  
**Alternatives considered:** Making the repo private (rejected: not possible for a GitHub fork without duplicating the repo under a new name); relying on developer discipline alone (rejected: no enforcement, too easy to accidentally stage the file).

---

<!-- Session 1eb37f56 — 2026-06-01 12:46 — review, edit, then move entries to decisions.md -->

## 2026-06-01 — LLM client reqwest timeout values and rationale

**Status:** decided  
**Context:** The reqwest client shared by post-processing and summarisation had no timeouts configured. Post-processing runs inline before paste, so a stalled LLM connection would block paste indefinitely; summarisation runs in the background but would hold a Tokio task open forever on a hung connection.  
**Decision:** Add `connect_timeout(10s)` and `timeout(120s)` to the reqwest `ClientBuilder`. 120s was chosen to accommodate slow or rate-limited LLM providers on large prompts while still bounding the worst case.  
**Consequences:** A stalled post-processing call now fails after at most 120s and surfaces as an error rather than a silent hang. Background summarisation tasks are similarly bounded. If a provider regularly takes more than 120s on large entries, the timeout will need raising — but that is preferable to an unbounded hang.  
**Alternatives considered:** Per-feature timeouts (rejected: both features share a single client instance; the values are appropriate for both); shorter timeout e.g. 30s (rejected: too aggressive for large-prompt summarisation on slower providers).

---

<!-- Session c5ae8283 — 2026-06-01 14:31 — review, edit, then move entries to decisions.md -->

## 2026-06-01 — Two-pipeline product mental model

**Status:** decided  
**Context:** Goldfish was growing beyond Handy's single-loop model (record → transcribe → paste). With summarisation added, two distinct output pipelines now exist that share a common capture/transcription stage but diverge at the point of use.  
**Decision:** The canonical mental model is: **Shared:** Capture → Transcribe → Post-process. **Pipeline A (immediate):** → Paste. **Pipeline B (deferred):** → Store → Review → Summarise. Post-processing is a shared enrichment step, not a paste-specific concern. The IA of settings, documentation, and future feature placement should reflect this split.  
**Consequences:** Post-processing belongs neither in Output nor in Summarisation — it sits in the shared pipeline and must be understood as pre-fork enrichment. Any future pipeline stage (e.g. tagging, routing) is evaluated against where it fits in this model, not which UI section it resembles.  
**Alternatives considered:** Post-processing as an Output concern (rejected: it produces enriched text consumed by both pipelines, not just paste); treating each pipeline as fully independent with duplicated settings (rejected: unnecessary complexity for shared config like provider/key).

---

## 2026-06-01 — Settings IA restructured to 9 pipeline-order sections

**Status:** decided  
**Context:** Handy's settings were structured around a single feature (transcription + paste) so grouping by UI concern was fine. Goldfish has multiple pipeline stages with independent configuration. The previous grouping (General, Models, Advanced, Post-processing, Summarisation, Debug) bundled shortcuts into feature sections and mixed pipeline-stage concerns with app-level concerns.  
**Decision:** Replace the previous section layout with 9 sections in pipeline order: **Shortcuts** (cross-cutting), **Capture** (mic, audio, clamshell), **Transcription** (model library, language, custom words — formerly "Models"), **Output** (paste behaviour, clipboard), **Post-processing** (enrichment toggle + prompt), **Summarisation** (model, prompt, enable), **App** (appearance, language, tray, updates), **About** (version, credits), **Debug** (dev-only). Section names reflect the user's pipeline stage, not internal implementation concepts.  
**Consequences:** Adding a new pipeline feature has a natural home without restructuring. "Models" disappears as a section name — the Transcription section owns model management. Shortcuts are not duplicated across sections. The order communicates the product's flow to users.  
**Alternatives considered:** Keep per-feature grouping (Post-processing and Summarisation as siblings in AI section) (rejected: conflates two pipeline stages with different timing and config); cross-cutting concern grouping (Capture, AI, Output as top-level with all features nested) (rejected: hides pipeline order which is the user's mental model).

---

## 2026-06-01 — Shortcuts extracted as first-class cross-cutting setup section

**Status:** decided  
**Context:** Keyboard shortcuts were previously embedded inside their respective feature sections (transcribe shortcut in General/Capture, post-process shortcut in Post-processing). As Goldfish adds features, each section would independently accumulate shortcut settings — creating a fragmented setup experience.  
**Decision:** All shortcuts (transcribe, cancel, push-to-talk, post-process hotkey) live in a single **Shortcuts** section at the top of settings navigation. No other section contains shortcut settings. This section is conceptually a "setup stage" — users visit it once during configuration, not repeatedly.  
**Consequences:** Future features that need a hotkey add their shortcut to the Shortcuts section only. A future onboarding flow can target this single section for first-run shortcut setup without scraping settings from multiple panels.  
**Alternatives considered:** Keep shortcuts in their respective feature sections (rejected: fragments setup experience and forces users to navigate multiple sections to configure hotkeys); shortcuts in a keyboard/input subsection of App settings (rejected: underweights setup importance; users configure shortcuts before they configure appearance).

---

## 2026-06-01 — Onboarding flow for shortcut setup deferred to backlog

**Status:** proposed  
**Context:** Extracting shortcuts into a dedicated section (see above) creates a clean target for a first-run onboarding flow. Goldfish requires keyboard shortcuts to operate — a user who never discovers or sets them cannot record anything. This is a known gap but was out of scope for the IA restructure.  
**Decision:** Log as a named backlog item (added to Notion: "Onboarding flow: keyboard shortcuts setup", P2, Feature + UX/UI). Implementation deferred. When built, the flow should trigger on first app launch and guide users through setting the transcribe shortcut as a minimum, with the Shortcuts settings section as the destination.  
**Trigger to revisit:** When Goldfish moves toward a first public release or beta; or when user research shows shortcut discovery is a friction point.  
**Consequences:** Until the onboarding flow exists, new users must discover shortcuts independently via settings. The Shortcuts section being first in the nav is a mitigation — it is visible without knowing where to look.  
**Alternatives considered:** Build minimal onboarding immediately alongside the IA restructure (rejected: out of scope; the IA restructure was already a large changeset); add a persistent first-run banner pointing at Shortcuts (rejected: deferred with the full onboarding scope rather than half-implementing a hint system).

---

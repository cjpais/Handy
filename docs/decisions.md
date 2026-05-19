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

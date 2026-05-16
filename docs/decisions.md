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

**Status:** proposed (not yet implemented in code)  

**Context:** Goldfish must be installable and identifiable as its own app, not an update channel for Handy.  

**Decision:** Use a new Tauri bundle identifier (e.g. `com.felixbaileymurray.goldfish`), `productName` “Goldfish”, own icons, and **disable or replace** Handy’s updater endpoint until Goldfish has its own release pipeline.  

**Consequences:** New app data directory; users do not inherit Handy settings/models automatically; can run beside Handy.  

**Alternatives considered:** Keep `com.pais.handy` to reuse data dir (rejected for a distinct product).

---

## 2026-05-16 — Documentation in `docs/`

**Status:** decided  

**Context:** Need transparent, local documentation to track analysis and decisions over time.  

**Decision:** Keep project-specific docs in `docs/` (`codebase-overview.md`, `fork-strategy.md`, `decisions.md`). Leave upstream docs at repo root (`README.md`, `AGENTS.md`, `BUILD.md`).  

**Consequences:** Update this file when making non-obvious choices; refresh overview when architecture changes materially after upstream merges.

---

<!-- Add new decisions below this line -->

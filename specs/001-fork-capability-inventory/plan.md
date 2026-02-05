# Implementation Plan: Fork Capability & Feature Inventory (Handy fork)

**Branch**: `001-fork-capability-inventory` | **Date**: February 5, 2026 | **Spec**: [specs/001-fork-capability-inventory/spec.md](specs/001-fork-capability-inventory/spec.md)
**Input**: Feature specification from `/specs/001-fork-capability-inventory/spec.md`

## Summary

Produce a markdown inventory of all user-visible UI surfaces (settings, onboarding, overlay, tray, permission prompts) with their commands/events/settings dependencies, Keep/Hide/Remove decisions, and guard points, plus a matching JSON capability object draft.

## Technical Context

**Language/Version**: Rust (edition 2021), TypeScript ~5.6, React 18.3, Tauri 2.9.1  
**Primary Dependencies**: Tauri 2.x, React, Vite 6.x, Tailwind CSS 4.x, tauri-specta 2.x  
**Storage**: N/A (documentation output committed to repo)  
**Testing**: N/A (documentation output); existing commands include `eslint` and `cargo test`  
**Target Platform**: Cross-platform desktop (Windows, macOS, Linux)  
**Project Type**: Desktop app with React frontend (src/) and Rust backend (src-tauri/)  
**Performance Goals**: N/A (documentation artifact)  
**Constraints**: Source of truth is current code/UI; prefer command/event map for dependencies; default Keep when uncertain  
**Scale/Scope**: Full inventory of user-visible surfaces in the current fork baseline

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

- No enforceable principles found in [.specify/memory/constitution.md](.specify/memory/constitution.md); file contains placeholders only. Gate passes by default.

## Project Structure

### Documentation (this feature)

```text
specs/001-fork-capability-inventory/
├── plan.md              # This file (/speckit.plan command output)
├── research.md          # Phase 0 output (/speckit.plan command)
├── data-model.md        # Phase 1 output (/speckit.plan command)
├── quickstart.md        # Phase 1 output (/speckit.plan command)
├── contracts/           # Phase 1 output (/speckit.plan command)
└── tasks.md             # Phase 2 output (/speckit.tasks command - NOT created by /speckit.plan)
```

### Source Code (repository root)

```text
src/                     # React/TypeScript UI
overlay/                 # Recording overlay UI
src-tauri/               # Rust backend (Tauri)
specs/                   # Feature specs and plans
```

**Structure Decision**: Use existing Tauri desktop layout with React frontend in src/, overlay UI in overlay/, and Rust backend in src-tauri/.

## Complexity Tracking

No constitution violations to justify.

## Phase 0: Outline & Research

- Research unknowns in technical context: None identified.
- Best practices research: Not required for documentation-only outputs.
- Consolidate decisions in [specs/001-fork-capability-inventory/research.md](specs/001-fork-capability-inventory/research.md).

## Phase 1: Design & Contracts

- Define entities and relationships in [specs/001-fork-capability-inventory/data-model.md](specs/001-fork-capability-inventory/data-model.md).
- Draft API contracts in [specs/001-fork-capability-inventory/contracts/capabilities.openapi.json](specs/001-fork-capability-inventory/contracts/capabilities.openapi.json).
- Create usage guide in [specs/001-fork-capability-inventory/quickstart.md](specs/001-fork-capability-inventory/quickstart.md).
- Update agent context via `.specify/scripts/powershell/update-agent-context.ps1 -AgentType copilot`.

**Constitution Re-check (Post-Design)**: No enforceable principles found in [.specify/memory/constitution.md](.specify/memory/constitution.md); gate remains pass.

## Phase 2: Planning

- Derive tasks from the inventory capture workflow (enumeration, dependency mapping, decisions, guard points, JSON draft).
- Ensure tasks include validation of item coverage and JSON parity with markdown.

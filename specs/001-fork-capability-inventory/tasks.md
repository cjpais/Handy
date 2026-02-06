---
description: "Task list for Fork Capability & Feature Inventory (Handy fork)"
---

# Tasks: Fork Capability & Feature Inventory (Handy fork)

**Input**: Design documents from `/specs/001-fork-capability-inventory/`
**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/

**Tests**: Not requested for this documentation-only feature.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (e.g., US1, US2, US3)
- Include exact file paths in descriptions

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: Prepare working documents for inventory capture

- [x] T001 Create draft worksheets in specs/001-fork-capability-inventory/drafts/settings.md, specs/001-fork-capability-inventory/drafts/onboarding.md, specs/001-fork-capability-inventory/drafts/navigation.md, specs/001-fork-capability-inventory/drafts/overlay.md, specs/001-fork-capability-inventory/drafts/tray.md, and specs/001-fork-capability-inventory/drafts/permissions.md
- [x] T002 Create capability inventory skeleton in specs/001-fork-capability-inventory/capability-inventory.md with scope and table headers
- [x] T003 Create JSON capability skeleton in specs/001-fork-capability-inventory/capabilities.json with version, generatedAt, scope, items, guardPoints

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Establish shared conventions that all story work depends on

- [x] T004 Define item id naming conventions and category rules in specs/001-fork-capability-inventory/capability-inventory.md
- [x] T005 Define decision, rationale, and missing-data notation rules in specs/001-fork-capability-inventory/capability-inventory.md
- [x] T006 Define guard point template section (id, description, coverage) in specs/001-fork-capability-inventory/capability-inventory.md

**Checkpoint**: Foundation ready - user story implementation can now begin

---

## Phase 3: User Story 1 - Review feature inventory for safe removal (Priority: P1) 🎯 MVP

**Goal**: Produce a complete inventory of user-visible UI surfaces with dependencies and decisions.

**Independent Test**: Review capability-inventory.md to confirm every surface has dependencies, decision, and rationale.

### Implementation for User Story 1

- [x] T007 [P] [US1] Populate settings inventory draft from src/components/settings/ in specs/001-fork-capability-inventory/drafts/settings.md
- [x] T008 [P] [US1] Populate onboarding inventory draft from src/components/onboarding/ in specs/001-fork-capability-inventory/drafts/onboarding.md
- [x] T009 [P] [US1] Populate navigation inventory draft from src/App.tsx and src/components/Sidebar.tsx in specs/001-fork-capability-inventory/drafts/navigation.md
- [x] T010 [P] [US1] Populate overlay inventory draft from overlay/ in specs/001-fork-capability-inventory/drafts/overlay.md
- [x] T011 [P] [US1] Populate tray menu inventory draft from src-tauri/src/tray.rs and src-tauri/src/tray_i18n.rs in specs/001-fork-capability-inventory/drafts/tray.md
- [x] T012 [P] [US1] Populate permission prompts inventory draft from src/components/AccessibilityPermissions.tsx and src-tauri/src/permissions-related modules in specs/001-fork-capability-inventory/drafts/permissions.md
- [x] T013 [US1] Consolidate drafts into specs/001-fork-capability-inventory/capability-inventory.md with commands, events, and settings keys per item
- [x] T014 [US1] Assign Keep/Hide/Remove decisions, rationales, and missing-data notes in specs/001-fork-capability-inventory/capability-inventory.md

**Checkpoint**: User Story 1 inventory is complete and reviewable

---

## Phase 4: User Story 2 - Identify single-flag guard points (Priority: P2)

**Goal**: Define guard points that disable related groups of items.

**Independent Test**: Verify each guard point lists covered items and a scope boundary.

### Implementation for User Story 2

- [x] T015 [US2] Define guard points with coverage lists in specs/001-fork-capability-inventory/capability-inventory.md
- [x] T016 [US2] Add guardPointId mappings for covered items in specs/001-fork-capability-inventory/capability-inventory.md

**Checkpoint**: Guard points are mapped to inventory items

---

## Phase 5: User Story 3 - Consume a capability JSON draft (Priority: P3)

**Goal**: Produce a JSON capability object draft that mirrors the inventory.

**Independent Test**: Validate every inventory item appears in the JSON with matching dependencies and decisions.

### Implementation for User Story 3

- [x] T017 [US3] Populate capabilities.json items array from specs/001-fork-capability-inventory/capability-inventory.md in specs/001-fork-capability-inventory/capabilities.json
- [x] T018 [US3] Populate capabilities.json guardPoints and metadata from specs/001-fork-capability-inventory/capability-inventory.md in specs/001-fork-capability-inventory/capabilities.json

**Checkpoint**: JSON draft mirrors the markdown inventory

---

## Phase 6: Polish & Cross-Cutting Concerns

**Purpose**: Validate completeness and parity across outputs

- [x] T019 Cross-check capability-inventory.md against README.md and architecture references; note any gaps in specs/001-fork-capability-inventory/capability-inventory.md
- [x] T020 Validate JSON parity with markdown inventory and fix mismatches in specs/001-fork-capability-inventory/capabilities.json

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: No dependencies - can start immediately
- **Foundational (Phase 2)**: Depends on Setup completion - BLOCKS all user stories
- **User Stories (Phase 3+)**: Depend on Foundational phase completion
- **Polish (Phase 6)**: Depends on all desired user stories being complete

### User Story Dependencies

- **User Story 1 (P1)**: Starts after Foundational
- **User Story 2 (P2)**: Depends on User Story 1 inventory
- **User Story 3 (P3)**: Depends on User Story 1 inventory and User Story 2 guard points

### Parallel Opportunities

- **User Story 1**: Draft worksheets T007-T012 can run in parallel (separate files)
- **User Story 2**: No parallel tasks (single inventory file)
- **User Story 3**: No parallel tasks (single JSON file)

---

## Parallel Example: User Story 1

```bash
Task: "Populate settings inventory draft" (specs/001-fork-capability-inventory/drafts/settings.md)
Task: "Populate onboarding inventory draft" (specs/001-fork-capability-inventory/drafts/onboarding.md)
Task: "Populate navigation inventory draft" (specs/001-fork-capability-inventory/drafts/navigation.md)
Task: "Populate overlay inventory draft" (specs/001-fork-capability-inventory/drafts/overlay.md)
Task: "Populate tray menu inventory draft" (specs/001-fork-capability-inventory/drafts/tray.md)
Task: "Populate permission prompts inventory draft" (specs/001-fork-capability-inventory/drafts/permissions.md)
```

---

## Implementation Strategy

### MVP First (User Story 1 Only)

1. Complete Phase 1: Setup
2. Complete Phase 2: Foundational
3. Complete Phase 3: User Story 1
4. Validate capability-inventory.md independently

### Incremental Delivery

1. Setup + Foundational → Foundation ready
2. User Story 1 → Validate inventory
3. User Story 2 → Validate guard points
4. User Story 3 → Validate JSON parity
5. Polish → Cross-check completeness

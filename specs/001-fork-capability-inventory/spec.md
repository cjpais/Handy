# Feature Specification: Fork Capability & Feature Inventory (Handy fork)

**Feature Branch**: `001-fork-capability-inventory`  
**Created**: February 5, 2026  
**Status**: Draft  
**Input**: User description: "Title: Fork Capability & Feature Inventory (Handy fork)\nGoal: Produce an inventory of UI surfaces and underlying backend dependencies so we can safely hide/remove features.\nInclude:\n- Table of routes/components, menu items, and settings panels\n- For each: commands invoked, events listened to, settings keys used\n- Mark Keep / Hide / Remove and the rationale\n- Identify \"guard points\" where a single flag can disable a whole area\nOutput: 1 markdown doc + a JSON capability object draft"

## Clarifications

### Session 2026-02-05

- Q: What UI surfaces are in scope for the inventory? → A: All user-visible UI surfaces: settings window, onboarding, recording overlay, tray menu, and platform permission prompts.
- Q: How should undecided items be treated? → A: Default Keep if uncertain; document missing info.
- Q: What is the minimum JSON capability object schema? → A: Standard: item id, name, category, decision, rationale, dependencies (commands/events/settings), guardPointId.
- Q: When dependencies are unclear, which source should be preferred? → A: Prefer the command/event map when available.
- Q: What should be the primary source of truth for inventory items? → A: Use current code/UI as source of truth; documentation only for cross-checking.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Review feature inventory for safe removal (Priority: P1)

As a product owner, I want a complete inventory of UI surfaces and their backend dependencies with Keep/Hide/Remove decisions so I can safely narrow the forked product scope.

**Why this priority**: This is the core decision-making artifact that drives which features stay, get hidden, or are removed.

**Independent Test**: Can be fully tested by reviewing the inventory document and confirming all required fields are present for every listed surface.

**Acceptance Scenarios**:

1. **Given** the current app UI surfaces, **When** the inventory is produced, **Then** it lists routes/components, menu items, and settings panels with dependencies and decisions.
2. **Given** a listed surface, **When** it is reviewed, **Then** it includes a Keep/Hide/Remove decision with a clear rationale.

---

### User Story 2 - Identify single-flag guard points (Priority: P2)

As a developer, I want guard points that can disable whole areas so I can implement feature flags efficiently.

**Why this priority**: Guard points reduce engineering effort and avoid inconsistent partial disabling.

**Independent Test**: Can be tested by checking that guard points map to multiple related items and describe the expected impact boundary.

**Acceptance Scenarios**:

1. **Given** a set of related UI surfaces, **When** guard points are identified, **Then** each guard point states which items it disables and the scope of impact.

---

### User Story 3 - Consume a capability JSON draft (Priority: P3)

As a system integrator, I want a JSON capability object draft that mirrors the inventory so I can use it in future tooling.

**Why this priority**: The JSON artifact enables automation without re-parsing the document.

**Independent Test**: Can be tested by validating that every inventory item appears in the JSON with matching decisions and dependencies.

**Acceptance Scenarios**:

1. **Given** the completed inventory, **When** the JSON draft is produced, **Then** it includes all items and guard points with consistent naming and decisions.

---

### Edge Cases

- A single backend dependency supports multiple UI surfaces with different Keep/Hide/Remove decisions.
- A UI surface has no documented commands, events, or settings keys.
- A surface appears in documentation but not in the current UI, or vice versa.
- Guard points overlap, creating conflicting or redundant disablement boundaries.

### Assumptions

- The inventory scope is limited to the current application UI, menus, and settings panels in the fork baseline.
- Dependencies are captured at the user-facing level (commands, events, settings keys) rather than internal implementation details.
- Scope includes settings window, onboarding, recording overlay, tray menu, and platform permission prompts; excludes backend-only commands with no UI surface.
- If a decision is uncertain, default to Keep and note the missing information.
- The JSON capability object uses a standard schema with item id, name, category, decision, rationale, dependencies (commands/events/settings), and guardPointId.
- When dependency information is unclear, prefer command/event mappings as the source of truth.
- Current code/UI is the primary source of truth; documentation is used only for cross-checking.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The system MUST produce a markdown inventory document listing routes/components, menu items, and settings panels. Acceptance: The document contains a table with those three categories represented.
- **FR-002**: Each inventory item MUST include commands invoked, events listened to, and settings keys used. Acceptance: Every item has values for all three dependency types or an explicit missing-data note.
- **FR-003**: Each inventory item MUST be labeled Keep, Hide, or Remove with a clear rationale. Acceptance: A decision and rationale are present for every item.
- **FR-004**: The inventory MUST identify guard points that can disable a whole area and list the items they cover. Acceptance: Each guard point lists its covered items and a scope boundary statement.
- **FR-005**: The system MUST provide a JSON capability object draft that mirrors the inventory content. Acceptance: The JSON contains the same item set and decisions as the markdown.
- **FR-006**: The inventory MUST explicitly note any items with missing dependency information. Acceptance: Missing data is labeled as such rather than left blank.
- **FR-007**: The inventory MUST define the scope boundaries for what is included and excluded. Acceptance: The document states inclusion and exclusion rules.

### Dependencies

- Access to the current app UI, menus, and settings panels.
- Access to existing documentation or source references for commands, events, and settings keys.

### Key Entities *(include if feature involves data)*

- **Inventory Item**: A UI surface or settings panel with dependencies, decision, and rationale.
- **Backend Dependency**: A command, event, or settings key referenced by an inventory item.
- **Capability Decision**: The Keep/Hide/Remove classification with rationale.
- **Guard Point**: A single control that disables a defined area with mapped items.
- **Capability Object**: The JSON representation of the inventory and guard points.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: 100% of known UI surfaces, menu items, and settings panels are listed with decisions.
- **SC-002**: 100% of listed items include commands, events, and settings keys or an explicit missing-data note.
- **SC-003**: Guard points cover at least 90% of items marked Hide or Remove.
- **SC-004**: The JSON capability object matches the markdown inventory with zero missing or extra items.

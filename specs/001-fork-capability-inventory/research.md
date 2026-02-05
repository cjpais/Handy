# Research: Fork Capability & Feature Inventory (Handy fork)

## Decisions

- Decision: Inventory scope includes settings window, onboarding, recording overlay, tray menu, and platform permission prompts.
  Rationale: This captures all user-visible surfaces that can be shown or hidden in the fork baseline.
  Alternatives considered: Settings-only scope; include backend-only commands with no UI surface.

- Decision: Current code/UI is the source of truth; documentation is cross-check only.
  Rationale: The inventory must reflect the real product surface and avoid stale docs.
  Alternatives considered: Documentation-only source; require code+doc agreement.

- Decision: Default Keep when uncertain and document missing information.
  Rationale: Avoids accidental removal while preserving visibility of gaps.
  Alternatives considered: Default Hide; default Remove; no default allowed.

- Decision: Prefer command/event mappings when dependency data is unclear.
  Rationale: IPC mappings are the most direct evidence of UI-backend linkage.
  Alternatives considered: Prefer settings keys; prefer docs.

- Decision: JSON capability schema uses item id, name, category, decision, rationale, dependencies (commands/events/settings), guardPointId.
  Rationale: Captures required decision and dependency data without overfitting implementation.
  Alternatives considered: Minimal schema without dependencies; extended schema with owners/risk.

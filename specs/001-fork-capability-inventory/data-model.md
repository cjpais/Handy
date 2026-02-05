# Data Model: Fork Capability & Feature Inventory (Handy fork)

## Entity: InventoryItem

- **Purpose**: Represents a user-visible UI surface or settings panel with dependencies and a decision.
- **Fields**:
  - `id` (string, required): Stable identifier, e.g., `settings.audio_feedback`.
  - `name` (string, required): Human-readable label.
  - `category` (enum, required): `route`, `component`, `menu`, `settings`, `overlay`, `permission`.
  - `location` (string, optional): Route or component path reference.
  - `decision` (enum, required): `keep`, `hide`, `remove`.
  - `rationale` (string, required): Reason for the decision.
  - `dependencies` (object, required): See `DependencySet`.
  - `guardPointId` (string, optional): Reference to a guard point that disables the item.
  - `notes` (string, optional): Missing-data or follow-up notes.
- **Validation**:
  - `decision` required with non-empty `rationale`.
  - `dependencies` arrays may be empty but must be present; use `notes` to flag missing data.

## Entity: DependencySet

- **Purpose**: Captures backend linkages for an inventory item.
- **Fields**:
  - `commands` (string[], required): Tauri command identifiers invoked.
  - `events` (string[], required): Tauri event names listened to.
  - `settingsKeys` (string[], required): Settings keys used by the UI surface.
- **Validation**:
  - At least one of the arrays may be empty; missing information must be noted in `InventoryItem.notes`.

## Entity: GuardPoint

- **Purpose**: A single flag or switch that disables a defined area.
- **Fields**:
  - `id` (string, required): Stable identifier, e.g., `guard.onboarding`.
  - `name` (string, required): Human-readable label.
  - `description` (string, required): Scope boundary statement.
  - `covers` (string[], required): `InventoryItem.id` values disabled by the guard point.
- **Validation**:
  - `covers` must reference existing `InventoryItem` entries.

## Entity: CapabilityObject

- **Purpose**: JSON container that mirrors the inventory.
- **Fields**:
  - `version` (string, required): Draft version string.
  - `generatedAt` (string, required): ISO-8601 timestamp.
  - `scope` (string, required): Human-readable scope boundary.
  - `items` (InventoryItem[], required)
  - `guardPoints` (GuardPoint[], required)
- **Validation**:
  - Every `InventoryItem.guardPointId` must map to a `GuardPoint.id`.

## Relationships

- `InventoryItem` → `GuardPoint`: Many items can map to one guard point.
- `CapabilityObject` aggregates all items and guard points.

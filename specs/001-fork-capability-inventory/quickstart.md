# Quickstart: Fork Capability & Feature Inventory (Handy fork)

## Goal

Produce a markdown inventory document plus a JSON capability object draft covering all user-visible UI surfaces.

## Scope

- Settings window
- Onboarding flow
- Recording overlay
- Tray menu
- Platform permission prompts

## Steps

1. **Enumerate UI surfaces** using current code/UI as the source of truth.
2. **Map dependencies** for each surface:
   - Commands invoked (Tauri `invoke` bindings)
   - Events listened to (Tauri event names)
   - Settings keys used (store-backed settings fields)
3. **Assign decisions** for each item:
   - `keep`, `hide`, or `remove`
   - Default to `keep` if uncertain and note missing data
4. **Identify guard points** that can disable multiple items with a single flag.
5. **Draft JSON capability object** using the standard schema.
6. **Cross-check documentation** only to ensure nothing obvious is missing.

## Outputs

- `capability-inventory.md` (markdown table of items and decisions)
- `capabilities.json` (JSON capability object draft)

## Validation Checklist

- Every item has commands/events/settings or a missing-data note
- Every item has decision + rationale
- Guard points list covered items and scope boundary
- JSON matches the markdown item set

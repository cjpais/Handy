# `src-tauri/capabilities/` — Tauri permission grants

Tauri 2 requires every frontend-accessible plugin or core API to be explicitly
granted per window. These JSON files are those grants. A missing permission shows
up at runtime as a rejected IPC call, not a type error — so this is the first place
to look when a frontend call fails with a permission denial.

See the [Tauri capabilities documentation](https://v2.tauri.app/security/capabilities/)
for the model. The `$schema` reference points at generated schemas in
[`../gen/schemas/`](../gen/schemas/), which Tauri regenerates — do not edit those.

---

## Files

### `default.json` — `default`

Grants the shared core and plugin surface to **both** windows
(`"windows": ["main", "recording_overlay"]`):

| Permission | Why it is needed |
| --- | --- |
| `core:default` | The baseline Tauri core API |
| `opener:default` | Opening the data/log/recordings folders and external links |
| `store:default` | Reading and writing `AppSettings` (tauri-plugin-store) |
| `updater:default` | In-app update checks |
| `process:default` | Relaunch/exit used by the updater flow |
| `dialog:default` | File pickers, e.g. choosing a custom sound or external script |
| `global-shortcut:allow-is-registered` / `allow-register` / `allow-unregister` / `allow-unregister-all` | The Tauri shortcut implementation; granted individually rather than as `default` |
| `macos-permissions:default` | Accessibility and microphone permission queries |
| `fs:read-files`, `fs:allow-resource-read-recursive` | Reading bundled resources and app data files |
| `fs:scope` allowing `$APPDATA` and `$APPDATA/**/*` | Confines filesystem reads to the app data directory |

The narrow `fs:scope` is deliberate: the frontend can read its own data directory
but not arbitrary paths. Widen it only with a specific reason.

### `desktop.json` — `desktop-capability`

Desktop-only and **`main` window only** (`"platforms": ["macOS", "windows",
"linux"]`):

- `autostart:default` — launch-at-login registration.
- `global-shortcut:default` — the full global-shortcut surface.
- `updater:default` — desktop update path.

---

## Conventions

- Grant the **narrowest** permission that works. Prefer named
  `allow-*` permissions over a plugin's `*:default` when only one capability is
  needed, as `default.json` does for `global-shortcut`.
- Scope filesystem access explicitly rather than granting broad read access.
- Keep window lists accurate. The overlay window needs relatively little; do not
  add it to a capability just because a call happens to work in development.
- Most app logic is **not** gated here: the ~120 custom Tauri commands are
  registered through tauri-specta in `lib.rs` and are callable without a
  capability entry. These files govern plugin and core permissions only.

---

## Notes

- `desktop.json` currently lists `autostart:default` three times. Duplicates are
  harmless (the permission set is a union) and this is upstream's content; left
  as-is to minimise merge friction. It is not a functional bug.
- Permissions are **not** the same as runtime OS permissions. macOS accessibility
  and microphone access, and Windows microphone privacy settings, are separate
  concerns surfaced through `AccessibilityPermissions.tsx`,
  `SecureInputWarning.tsx` and the `commands::audio` permission commands.
- Changing a plugin's feature set in `Cargo.toml` may require a matching
  permission entry here; a compile succeeds but the IPC call is denied at runtime.

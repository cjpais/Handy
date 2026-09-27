# `src-tauri/nsis/` — Windows installer template

A custom NSIS installer script for the Windows build. Tauri uses this file instead
of its built-in template because upstream Handy adds **portable mode** support to
the installer.

Wired up in `tauri.conf.json`:

```json
"windows": { "nsis": { "template": "nsis/installer.nsi" } }
```

---

## File

`installer.nsi` — derived from Tauri v2's
`crates/tauri-bundler/src/bundle/windows/nsis/installer.nsi`, with Handy's
additions marked by `; --- PORTABLE MODE ---` comments. The header records the
upstream version it was based on (`tauri-v2.9.1`) — useful when diffing.

The portable additions let the installer set up a directory that runs without a
traditional install, matching the runtime behaviour in
[`../src/portable.rs`](../src/portable.rs): a `portable` marker file next to the
executable redirects all user data (settings, models, recordings, database, logs)
into `./Data/`.

---

## Maintaining this file

The header comment states the required workflow:

> When upgrading Tauri, diff this file against the new upstream template and merge
> changes while preserving the portable sections.

The template is a **vendored fork of Tauri's**, so it does not update itself. After
a Tauri version bump, the installer may silently fall behind upstream fixes. Treat
`installer.nsi` as a file to check on every Tauri upgrade, and keep the
`; --- PORTABLE MODE ---` markers intact so the diff stays reviewable.

---

## Notes

- **Building the installer requires NSIS**, which is platform-specific. On
  non-Windows hosts, or to skip installer signing entirely, use
  `bun run tauri build --no-bundle`.
- Windows release signing uses a `signCommand` in `tauri.conf.json` that points at
  upstream's Azure Trusted Signing account. It is CI-only and will fail locally.
  See [`../README.md`](../README.md) for the distribution-identity checklist.
- Changing portable-mode behaviour requires edits in **two** places that must stay
  consistent: this installer (which decides what gets laid down) and
  `src/portable.rs` (which decides what the running app does with it). The marker
  file's magic string is defined on the Rust side.
- The runtime side treats an *empty* marker as invalid unless a `Data/` directory
  already exists (a legacy-migration case), so an installer writing the marker must
  write the correct magic string, not an empty file.

# `nix/` — NixOS and Home Manager modules

Nix modules for installing Handy on NixOS and for running it as a user service.
Referenced by the repository's `flake.nix`.

These modules only cover what a package wrapper **cannot** do. The package itself
is defined in `flake.nix`; these add system-level integration.

---

## Files

### `module.nix` — NixOS module

Enables `programs.handy`.

Its stated reason for existing is a **udev rule for `/dev/uinput`**, which `rdev`'s
`grab()` needs to create virtual input. A plain package install cannot provide that.

```nix
{
  imports = [ handy.nixosModules.default ];
  programs.handy.enable = true;
}
```

> **Users must add themselves to the `input` group** to get evdev hotkey access.
> The module does not do this for you, and hotkeys will not work without it.

### `hm-module.nix` — Home Manager module

Enables `services.handy` as a **systemd user service** for autostart.

```nix
{
  imports = [ handy.homeManagerModules.default ];
  services.handy.enable = true;
}
```

The unit starts `After`/`PartOf` `graphical-session.target`, runs
`${cfg.package}/bin/handy`, and restarts on failure after 5 seconds, with
`WantedBy = [ "graphical-session.target" ]`.

Because the service autostarts the app, the in-app autostart setting becomes
redundant on NixOS — pick one mechanism rather than both.

---

## Self-update is disabled under Nix

The Nix package sets **`HANDY_DISABLE_UPDATER=1`**, which force-disables the
in-app updater at runtime without touching the persisted setting. Self-update
cannot work against an immutable `/nix/store`, so allowing it would only produce a
confusing failure. `commands::is_update_checks_locked` reports this state to the
frontend, which is why the update toggle can appear locked.

---

## Maintainer notes

- `.nix/bun.nix` at the repository root is **generated** from `bun.lock` by
  [`../scripts/check-nix-deps.ts`](../scripts/check-nix-deps.ts) (runs on
  `bun install`). The Nix build consumes it. If you update `bun.lock`, commit
  `.nix/bun.nix` and `.nix/bun-lock-hash` too, or CI's `nix-check` will fail.
- The Nix build vendors Cargo dependencies via `importCargoLock`, which is why
  `Cargo.toml` patches `tao-macros` to the **same git rev** as `tao` — two entries
  for the same crate version cause a symlink collision there. See the comment block
  in [`../src-tauri/Cargo.toml`](../src-tauri/Cargo.toml). Do not remove that patch.
- `nix/` is the module directory; `.nix/` (dot-prefixed) is separate generated
  dependency data. Do not confuse them.
- Wayland support is limited, and the overlay uses GTK layer shell. On
  compositors where that is unavailable it falls back to a regular window; disable
  it explicitly with `HANDY_NO_GTK_LAYER_SHELL=1`.

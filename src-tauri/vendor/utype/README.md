# Vendored utype

Upstream: https://github.com/vanviegen/utype (MIT, see `LICENSE`)
Pinned commit: `70178086dbbbadaeb82ee48efb8d699e685d0cd8`

`utype` speaks every fake-keyboard-input protocol Linux desktops offer — the
Wayland virtual-keyboard and KDE fake-input protocols, Mutter's and Muffin's
remote-desktop D-Bus APIs, uinput via `ydotoold`, and XTEST on X11 — with no
link-time dependencies beyond libc. `src-tauri/build.rs` compiles these sources
into `resources/utype`, which Handy bundles and runs as a child process
(see `src-tauri/src/utype.rs`).

The sources are copied verbatim; do not patch them here. Send fixes upstream
and pull them in with `scripts/update-utype.sh`.

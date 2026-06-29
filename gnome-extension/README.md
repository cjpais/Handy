# Handy GNOME Status Extension

This optional GNOME Shell extension shows Handy's live activity state in the top bar using Handy's user-session D-Bus status API.

## Install

```bash
./gnome-extension/install.sh
```

The script packs, installs, and enables the extension via the `gnome-extensions` CLI. On GNOME Wayland, log out and back in so GNOME Shell loads it (disable/enable does not reload cached modules on Wayland).

## D-Bus Checks

```bash
busctl --user introspect com.pais.Handy /com/pais/Handy
busctl --user call com.pais.Handy /com/pais/Handy com.pais.Handy.Status GetStatus
busctl --user monitor com.pais.Handy
```

The extension is status-only. It does not create windows, request focus, read the clipboard, or trigger input injection.

# Handy patch to global-hotkey 0.8.0

Source: the crates.io release of `global-hotkey` 0.8.0 from
<https://github.com/tauri-apps/global-hotkey>.

Accept `WakeUp` and `XF86WakeUp` shortcut names and map `Code::WakeUp` to the
native X11 keysym. Some ThinkPad keyboards report their Fn key this way.
Press and release events use the existing global shortcut handler, including
Handy's Auto activation mode. The key remains named WakeUp because other
hardware can use the same event for a dedicated wake key.

The source and license files are retained. Examples and their development
dependencies are omitted from this application-local copy.

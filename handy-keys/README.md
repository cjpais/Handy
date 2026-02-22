# handy-keys

Cross-platform global keyboard shortcuts library for Rust.

## Features

- **Cross-platform**: Works on macOS, Windows, and Linux
- **Global hotkeys**: Register system-wide keyboard shortcuts
- **Hotkey blocking**: Registered hotkeys are blocked from reaching other applications
- **Modifier-only hotkeys**: Support for shortcuts like `Cmd+Shift` without a key
- **String parsing**: Parse hotkeys from strings like `"Ctrl+Alt+Space"`
- **Hotkey recording**: Low-level keyboard listener for "record a hotkey" UI flows
- **Serde support**: All types implement `Serialize`/`Deserialize`

## Installation

```toml
[dependencies]
handy-keys = "0.1"
```

## Quick Start

```rust
use handy_keys::{HotkeyManager, Hotkey, Modifiers, Key};

fn main() -> handy_keys::Result<()> {
    let manager = HotkeyManager::new()?;

    // Register using the type-safe constructor
    let hotkey = Hotkey::new(Modifiers::CMD | Modifiers::SHIFT, Key::K)?;
    let id = manager.register(hotkey)?;

    // Or parse from a string
    let hotkey2: Hotkey = "Ctrl+Alt+Space".parse()?;
    manager.register(hotkey2)?;

    // Listen for events
    while let Ok(event) = manager.recv() {
        println!("Hotkey triggered: {:?}", event.id);
    }

    Ok(())
}
```

## Platform Notes

### macOS

Requires accessibility permissions. The library provides helpers to check and request access:

```rust
use handy_keys::{check_accessibility, open_accessibility_settings};

if !check_accessibility() {
    open_accessibility_settings()?;
}
```

### Windows

Uses low-level keyboard hooks. No special permissions required.

### Linux

Uses [rdev](https://crates.io/crates/rdev). On Wayland, hotkey blocking may not work due to compositor restrictions.

## Modifiers

| Modifier | Aliases |
|----------|---------|
| `CMD` | `command`, `meta`, `super`, `win` |
| `CTRL` | `control` |
| `OPT` | `option`, `alt` |
| `SHIFT` | |
| `FN` | `function` (macOS only) |

## Recording Hotkeys

For implementing "press a key to set hotkey" UI:

```rust
use handy_keys::KeyboardListener;

let listener = KeyboardListener::new()?;

println!("Press a key combination...");
while let Ok(event) = listener.recv() {
    if event.is_key_down {
        if let Ok(hotkey) = event.as_hotkey() {
            println!("Recorded: {}", hotkey);
            break;
        }
    }
}
```

## License

MIT

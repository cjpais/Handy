//! Cross-platform global keyboard shortcuts library.
//!
//! `handy-keys` provides a simple way to register and listen for global keyboard
//! shortcuts across macOS, Windows, and Linux.
//!
//! # Features
//!
//! - **Global hotkeys**: Register system-wide keyboard shortcuts that work even
//!   when your application is not focused
//! - **Hotkey blocking**: Registered hotkeys are blocked from reaching other applications
//! - **Modifier-only hotkeys**: Support for shortcuts like `Cmd+Shift` without a key
//! - **String parsing**: Parse hotkeys from strings like `"Ctrl+Alt+Space"`
//! - **Hotkey recording**: Low-level [`KeyboardListener`] for implementing
//!   "record a hotkey" UI flows
//! - **Serde support**: All types implement `Serialize`/`Deserialize`
//!
//! # Quick Start
//!
//! ```no_run
//! use handy_keys::{HotkeyManager, Hotkey, Modifiers, Key};
//!
//! fn main() -> handy_keys::Result<()> {
//!     let manager = HotkeyManager::new()?;
//!
//!     // Register Cmd+Shift+K using the type-safe constructor
//!     let hotkey = Hotkey::new(Modifiers::CMD | Modifiers::SHIFT, Key::K)?;
//!     let id = manager.register(hotkey)?;
//!
//!     // Or parse from a string (useful for UI/config input)
//!     let hotkey2: Hotkey = "Ctrl+Alt+Space".parse()?;
//!     let id2 = manager.register(hotkey2)?;
//!
//!     println!("Registered hotkeys: {:?}, {:?}", id, id2);
//!
//!     // Wait for hotkey events
//!     while let Ok(event) = manager.recv() {
//!         println!("Hotkey triggered: {:?}", event.id);
//!     }
//!
//!     Ok(())
//! }
//! ```
//!
//! # Recording Hotkeys
//!
//! For implementing "press a key to set hotkey" UIs, use [`KeyboardListener`]:
//!
//! ```no_run
//! use handy_keys::KeyboardListener;
//!
//! let listener = KeyboardListener::new()?;
//!
//! // Listen for key events
//! while let Ok(event) = listener.recv() {
//!     if event.is_key_down {
//!         if let Ok(hotkey) = event.as_hotkey() {
//!             println!("User pressed: {}", hotkey);
//!             break;
//!         }
//!     }
//! }
//! # Ok::<(), handy_keys::Error>(())
//! ```
//!
//! # Platform Notes
//!
//! ## macOS
//!
//! Requires accessibility permissions. Use [`check_accessibility`] to check if
//! permissions are granted, and [`open_accessibility_settings`] to prompt the user:
//!
//! ```no_run
//! # #[cfg(target_os = "macos")]
//! # fn main() -> handy_keys::Result<()> {
//! use handy_keys::{check_accessibility, open_accessibility_settings};
//!
//! if !check_accessibility() {
//!     open_accessibility_settings()?;
//!     // User needs to grant permission and restart
//! }
//! # Ok(())
//! # }
//! # #[cfg(not(target_os = "macos"))]
//! # fn main() {}
//! ```
//!
//! ## Windows
//!
//! Uses low-level keyboard hooks. No special permissions required.
//!
//! ## Linux
//!
//! Uses [rdev](https://crates.io/crates/rdev). On Wayland, hotkey blocking may not
//! work due to compositor restrictions.

mod error;
mod listener;
mod manager;
mod platform;
mod types;

pub use error::{Error, Result};
pub use listener::{BlockingHotkeys, KeyboardListener};
pub use manager::HotkeyManager;
pub use types::{Hotkey, HotkeyEvent, HotkeyId, HotkeyState, Key, KeyEvent, Modifiers};

#[cfg(target_os = "macos")]
pub use platform::macos::{check_accessibility, open_accessibility_settings};

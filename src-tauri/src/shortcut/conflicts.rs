//! Keyboard shortcut conflict detection
//!
//! Detects conflicts between user-defined shortcuts and platform-specific
//! reserved system shortcuts. Provides warnings without blocking registration.

use serde::Serialize;
use specta::Type;

/// Information about a detected shortcut conflict.
#[derive(Debug, Clone, Serialize, Type)]
pub struct ConflictInfo {
    /// The platform where this conflict applies (e.g., "macOS", "Windows", "Linux")
    pub platform: String,
    /// Human-readable name of the reserved shortcut (e.g., "Quit Application")
    pub name: String,
    /// Whether this shortcut is fully reserved by the system (cannot be overridden)
    /// vs. merely a common shortcut that may conflict (can be overridden)
    pub reserved: bool,
}

/// A reserved shortcut entry for conflict checking.
struct ReservedShortcut {
    /// Normalized shortcut string (e.g., "cmd+q", "alt+f4")
    shortcut: &'static str,
    /// Human-readable description
    name: &'static str,
    /// Whether the shortcut is fully reserved (system intercepts it) vs. merely conflicting
    reserved: bool,
}

// ── macOS reserved shortcuts ──────────────────────────────────────────

#[cfg(target_os = "macos")]
const MACOS_RESERVED: &[ReservedShortcut] = &[
    ReservedShortcut {
        shortcut: "cmd+q",
        name: "Quit Application",
        reserved: true,
    },
    ReservedShortcut {
        shortcut: "cmd+w",
        name: "Close Window",
        reserved: false,
    },
    ReservedShortcut {
        shortcut: "cmd+h",
        name: "Hide Application",
        reserved: true,
    },
    ReservedShortcut {
        shortcut: "cmd+m",
        name: "Minimize Window",
        reserved: false,
    },
    ReservedShortcut {
        shortcut: "cmd+space",
        name: "Spotlight Search",
        reserved: true,
    },
    ReservedShortcut {
        shortcut: "cmd+tab",
        name: "Application Switcher",
        reserved: true,
    },
    ReservedShortcut {
        shortcut: "cmd+shift+3",
        name: "Screenshot (Full Screen)",
        reserved: true,
    },
    ReservedShortcut {
        shortcut: "cmd+shift+4",
        name: "Screenshot (Selection)",
        reserved: true,
    },
    ReservedShortcut {
        shortcut: "cmd+shift+5",
        name: "Screenshot/Recording Utility",
        reserved: true,
    },
    ReservedShortcut {
        shortcut: "cmd+option+esc",
        name: "Force Quit",
        reserved: true,
    },
    ReservedShortcut {
        shortcut: "cmd+comma",
        name: "Preferences",
        reserved: false,
    },
    ReservedShortcut {
        shortcut: "cmd+n",
        name: "New Window",
        reserved: false,
    },
    ReservedShortcut {
        shortcut: "cmd+s",
        name: "Save",
        reserved: false,
    },
    ReservedShortcut {
        shortcut: "ctrl+arrow_up",
        name: "Mission Control",
        reserved: true,
    },
    ReservedShortcut {
        shortcut: "ctrl+arrow_down",
        name: "Application Windows",
        reserved: true,
    },
    ReservedShortcut {
        shortcut: "ctrl+arrow_left",
        name: "Switch Desktop Left",
        reserved: true,
    },
    ReservedShortcut {
        shortcut: "ctrl+arrow_right",
        name: "Switch Desktop Right",
        reserved: true,
    },
    // Also match with "control" alias
    ReservedShortcut {
        shortcut: "control+arrow_up",
        name: "Mission Control",
        reserved: true,
    },
    ReservedShortcut {
        shortcut: "control+arrow_down",
        name: "Application Windows",
        reserved: true,
    },
    ReservedShortcut {
        shortcut: "control+arrow_left",
        name: "Switch Desktop Left",
        reserved: true,
    },
    ReservedShortcut {
        shortcut: "control+arrow_right",
        name: "Switch Desktop Right",
        reserved: true,
    },
];

#[cfg(not(target_os = "macos"))]
const MACOS_RESERVED: &[ReservedShortcut] = &[];

// ── Windows reserved shortcuts ────────────────────────────────────────

#[cfg(target_os = "windows")]
const WINDOWS_RESERVED: &[ReservedShortcut] = &[
    ReservedShortcut {
        shortcut: "alt+f4",
        name: "Close Window",
        reserved: true,
    },
    ReservedShortcut {
        shortcut: "ctrl+alt+delete",
        name: "System Security / Task Manager",
        reserved: true,
    },
    ReservedShortcut {
        shortcut: "ctrl+alt+del",
        name: "System Security / Task Manager",
        reserved: true,
    },
    ReservedShortcut {
        shortcut: "win+e",
        name: "File Explorer",
        reserved: true,
    },
    ReservedShortcut {
        shortcut: "win+d",
        name: "Show Desktop",
        reserved: true,
    },
    ReservedShortcut {
        shortcut: "win+l",
        name: "Lock Screen",
        reserved: true,
    },
    ReservedShortcut {
        shortcut: "win+tab",
        name: "Task View",
        reserved: true,
    },
    ReservedShortcut {
        shortcut: "win+r",
        name: "Run Dialog",
        reserved: true,
    },
    ReservedShortcut {
        shortcut: "win+i",
        name: "Settings",
        reserved: true,
    },
    ReservedShortcut {
        shortcut: "win+s",
        name: "Search",
        reserved: true,
    },
    ReservedShortcut {
        shortcut: "alt+tab",
        name: "Window Switcher",
        reserved: true,
    },
    ReservedShortcut {
        shortcut: "ctrl+shift+esc",
        name: "Task Manager",
        reserved: true,
    },
    ReservedShortcut {
        shortcut: "win+shift+s",
        name: "Screenshot",
        reserved: true,
    },
    ReservedShortcut {
        shortcut: "win+v",
        name: "Clipboard History",
        reserved: false,
    },
    ReservedShortcut {
        shortcut: "win+.",
        name: "Emoji Picker",
        reserved: false,
    },
    ReservedShortcut {
        shortcut: "win+;",
        name: "Emoji Picker",
        reserved: false,
    },
];

#[cfg(not(target_os = "windows"))]
#[allow(dead_code)]
const WINDOWS_RESERVED: &[ReservedShortcut] = &[];

// ── Linux reserved shortcuts ───────────────────────────────────────────
// Note: Linux DE shortcuts vary widely. We check for the most common ones
// across GNOME and KDE.

#[cfg(target_os = "linux")]
const LINUX_RESERVED: &[ReservedShortcut] = &[
    // GNOME shortcuts
    ReservedShortcut {
        shortcut: "super",
        name: "Activities Overview (GNOME)",
        reserved: true,
    },
    ReservedShortcut {
        shortcut: "alt+tab",
        name: "Window Switcher",
        reserved: true,
    },
    ReservedShortcut {
        shortcut: "ctrl+alt+tab",
        name: "Switch System Controls",
        reserved: true,
    },
    ReservedShortcut {
        shortcut: "ctrl+alt+arrow_up",
        name: "Switch Workspace Up (GNOME)",
        reserved: true,
    },
    ReservedShortcut {
        shortcut: "ctrl+alt+arrow_down",
        name: "Switch Workspace Down (GNOME)",
        reserved: true,
    },
    ReservedShortcut {
        shortcut: "ctrl+alt+left",
        name: "Switch Workspace Left (GNOME)",
        reserved: true,
    },
    ReservedShortcut {
        shortcut: "ctrl+alt+right",
        name: "Switch Workspace Right (GNOME)",
        reserved: true,
    },
    ReservedShortcut {
        shortcut: "super+l",
        name: "Lock Screen (GNOME)",
        reserved: true,
    },
    ReservedShortcut {
        shortcut: "super+d",
        name: "Show Desktop (GNOME)",
        reserved: false,
    },
    ReservedShortcut {
        shortcut: "super+v",
        name: "Notifications (GNOME)",
        reserved: false,
    },
    ReservedShortcut {
        shortcut: "alt+f2",
        name: "Run Command (GNOME)",
        reserved: true,
    },
    ReservedShortcut {
        shortcut: "ctrl+shift+esc",
        name: "System Monitor (GNOME)",
        reserved: false,
    },
    // KDE shortcuts
    ReservedShortcut {
        shortcut: "alt+f4",
        name: "Close Window (KDE)",
        reserved: true,
    },
    ReservedShortcut {
        shortcut: "ctrl+f1",
        name: "Desktop 1 (KDE)",
        reserved: false,
    },
    ReservedShortcut {
        shortcut: "meta+tab",
        name: "Present Windows (KDE)",
        reserved: false,
    },
];

#[cfg(not(target_os = "linux"))]
#[allow(dead_code)]
const LINUX_RESERVED: &[ReservedShortcut] = &[];

/// Normalize a shortcut string for comparison.
///
/// Converts to lowercase and sorts modifier keys into a consistent order
/// so that "Ctrl+Alt+A" and "alt+ctrl+a" match.
pub fn normalize_shortcut(shortcut: &str) -> String {
    let parts: Vec<String> = shortcut
        .split('+')
        .map(|p| p.trim().to_lowercase())
        .collect();

    // Modifier key order for normalization
    let modifier_order = [
        "ctrl", "control", "shift", "alt", "option", "meta", "cmd", "command", "super", "win",
        "windows", "fn",
    ];

    let mut modifiers: Vec<String> = Vec::new();
    let mut non_modifiers: Vec<String> = Vec::new();

    for part in parts {
        if modifier_order.contains(&part.as_str()) {
            modifiers.push(part);
        } else {
            non_modifiers.push(part);
        }
    }

    // Sort modifiers for consistent ordering
    modifiers.sort_by_key(|m| {
        modifier_order
            .iter()
            .position(|&o| o == m.as_str())
            .unwrap_or(999)
    });

    let mut all = modifiers;
    all.extend(non_modifiers);
    all.join("+")
}

/// Detect conflicts between a user-defined shortcut and platform-specific reserved shortcuts.
///
/// Returns a list of `ConflictInfo` entries describing any conflicts found.
/// The shortcut string should be in the standard format (e.g., "cmd+option+space").
///
/// This function checks against the current platform's reserved shortcuts only.
/// It does NOT block registration — the caller should use this to emit warnings.
pub fn detect_conflicts_for_shortcut(shortcut: &str) -> Vec<ConflictInfo> {
    let normalized = normalize_shortcut(shortcut);
    let mut conflicts = Vec::new();

    // Check platform-specific conflicts
    let reserved_list: &[ReservedShortcut] = {
        #[cfg(target_os = "macos")]
        {
            MACOS_RESERVED
        }
        #[cfg(target_os = "windows")]
        {
            WINDOWS_RESERVED
        }
        #[cfg(target_os = "linux")]
        {
            LINUX_RESERVED
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
        {
            &[]
        }
    };

    let platform_name = {
        #[cfg(target_os = "macos")]
        {
            "macOS"
        }
        #[cfg(target_os = "windows")]
        {
            "Windows"
        }
        #[cfg(target_os = "linux")]
        {
            "Linux"
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
        {
            "Unknown"
        }
    };

    for entry in reserved_list {
        if normalized == normalize_shortcut(entry.shortcut) {
            conflicts.push(ConflictInfo {
                platform: platform_name.to_string(),
                name: entry.name.to_string(),
                reserved: entry.reserved,
            });
        }
    }

    conflicts
}

/// Check all platform reserved shortcuts (useful for cross-platform settings UI).
///
/// Returns conflicts from all platforms, regardless of the current OS.
/// Useful for showing the user what would happen on other platforms.
#[allow(dead_code)]
pub fn detect_conflicts_all_platforms(shortcut: &str) -> Vec<ConflictInfo> {
    let normalized = normalize_shortcut(shortcut);
    let mut conflicts = Vec::new();

    for entry in MACOS_RESERVED {
        if normalized == normalize_shortcut(entry.shortcut) {
            conflicts.push(ConflictInfo {
                platform: "macOS".to_string(),
                name: entry.name.to_string(),
                reserved: entry.reserved,
            });
        }
    }

    for entry in WINDOWS_RESERVED {
        if normalized == normalize_shortcut(entry.shortcut) {
            conflicts.push(ConflictInfo {
                platform: "Windows".to_string(),
                name: entry.name.to_string(),
                reserved: entry.reserved,
            });
        }
    }

    for entry in LINUX_RESERVED {
        if normalized == normalize_shortcut(entry.shortcut) {
            conflicts.push(ConflictInfo {
                platform: "Linux".to_string(),
                name: entry.name.to_string(),
                reserved: entry.reserved,
            });
        }
    }

    conflicts
}

/// Tauri command wrapper for conflict detection.
///
/// Accepts the shortcut string as an owned `String` (Tauri commands cannot
/// take references) and delegates to [`detect_conflicts_for_shortcut`].
#[tauri::command]
#[specta::specta]
pub fn detect_conflicts(shortcut: String) -> Vec<ConflictInfo> {
    detect_conflicts_for_shortcut(&shortcut)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_shortcut_basic() {
        assert_eq!(normalize_shortcut("Cmd+Q"), "cmd+q");
        assert_eq!(normalize_shortcut("ctrl+alt+delete"), "ctrl+alt+delete");
    }

    #[test]
    fn test_normalize_shortcut_reorder_modifiers() {
        // Modifiers should be sorted into consistent order
        assert_eq!(normalize_shortcut("alt+ctrl+a"), "ctrl+alt+a");
        assert_eq!(normalize_shortcut("a+ctrl+alt"), "ctrl+alt+a");
    }

    #[test]
    fn test_normalize_shortcut_spaces() {
        assert_eq!(normalize_shortcut("Cmd + Q"), "cmd+q");
        assert_eq!(normalize_shortcut(" ctrl + shift + s "), "ctrl+shift+s");
    }

    #[test]
    fn test_detect_conflicts_macos() {
        #[cfg(target_os = "macos")]
        {
            let conflicts = detect_conflicts("cmd+q".to_string());
            assert!(!conflicts.is_empty());
            assert!(conflicts[0].reserved);
            assert_eq!(conflicts[0].name, "Quit Application");
        }

        #[cfg(not(target_os = "macos"))]
        {
            let conflicts = detect_conflicts("cmd+q".to_string());
            assert!(conflicts.is_empty());
        }
    }

    #[test]
    fn test_detect_no_conflict() {
        let conflicts = detect_conflicts("ctrl+shift+f12".to_string());
        assert!(conflicts.is_empty());
    }

    #[test]
    fn test_detect_conflicts_all_platforms() {
        // Alt+F4 should conflict on Windows and Linux
        let conflicts = detect_conflicts_all_platforms("alt+f4");
        let platforms: Vec<&str> = conflicts.iter().map(|c| c.platform.as_str()).collect();
        assert!(platforms.contains(&"Windows") || platforms.contains(&"Linux"));
    }

    #[test]
    fn test_detect_conflicts_cmd_space() {
        let all = detect_conflicts_all_platforms("cmd+space");
        assert!(!all.is_empty());
        assert!(all
            .iter()
            .any(|c| c.platform == "macOS" && c.name == "Spotlight Search"));
    }
}
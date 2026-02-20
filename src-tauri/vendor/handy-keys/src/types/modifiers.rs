//! Modifier key definitions and parsing

use bitflags::bitflags;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

use crate::error::{Error, Result};

bitflags! {
    /// Modifier keys for hotkey combinations
    ///
    /// Individual flags track which side (left/right) was pressed.
    /// Compound aliases (`CMD`, `SHIFT`, etc.) match either side.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
    #[serde(transparent)]
    pub struct Modifiers: u32 {
        // Individual side-specific flags
        const CMD_LEFT    = 1 << 0;
        const SHIFT_LEFT  = 1 << 1;
        const CTRL_LEFT   = 1 << 2;
        const OPT_LEFT    = 1 << 3;
        const FN          = 1 << 4;
        const CMD_RIGHT   = 1 << 5;
        const SHIFT_RIGHT = 1 << 6;
        const CTRL_RIGHT  = 1 << 7;
        const OPT_RIGHT   = 1 << 8;

        // Compound aliases — "either side"
        const CMD   = Self::CMD_LEFT.bits()   | Self::CMD_RIGHT.bits();
        const SHIFT = Self::SHIFT_LEFT.bits() | Self::SHIFT_RIGHT.bits();
        const CTRL  = Self::CTRL_LEFT.bits()  | Self::CTRL_RIGHT.bits();
        const OPT   = Self::OPT_LEFT.bits()   | Self::OPT_RIGHT.bits();
    }
}

/// Helper: all modifier groups as (left, right, compound) triples.
const GROUPS: [(Modifiers, Modifiers, Modifiers); 4] = [
    (Modifiers::CMD_LEFT, Modifiers::CMD_RIGHT, Modifiers::CMD),
    (
        Modifiers::SHIFT_LEFT,
        Modifiers::SHIFT_RIGHT,
        Modifiers::SHIFT,
    ),
    (Modifiers::CTRL_LEFT, Modifiers::CTRL_RIGHT, Modifiers::CTRL),
    (Modifiers::OPT_LEFT, Modifiers::OPT_RIGHT, Modifiers::OPT),
];

impl fmt::Display for Modifiers {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut parts = Vec::new();

        // Ctrl group
        if self.contains(Modifiers::CTRL) {
            parts.push("Ctrl");
        } else if self.contains(Modifiers::CTRL_LEFT) {
            parts.push("CtrlLeft");
        } else if self.contains(Modifiers::CTRL_RIGHT) {
            parts.push("CtrlRight");
        }

        // Opt group
        if self.contains(Modifiers::OPT) {
            parts.push("Opt");
        } else if self.contains(Modifiers::OPT_LEFT) {
            parts.push("OptLeft");
        } else if self.contains(Modifiers::OPT_RIGHT) {
            parts.push("OptRight");
        }

        // Shift group
        if self.contains(Modifiers::SHIFT) {
            parts.push("Shift");
        } else if self.contains(Modifiers::SHIFT_LEFT) {
            parts.push("ShiftLeft");
        } else if self.contains(Modifiers::SHIFT_RIGHT) {
            parts.push("ShiftRight");
        }

        // Cmd group
        if self.contains(Modifiers::CMD) {
            parts.push("Cmd");
        } else if self.contains(Modifiers::CMD_LEFT) {
            parts.push("CmdLeft");
        } else if self.contains(Modifiers::CMD_RIGHT) {
            parts.push("CmdRight");
        }

        // Fn
        if self.contains(Modifiers::FN) {
            parts.push("Fn");
        }

        write!(f, "{}", parts.join("+"))
    }
}

impl Modifiers {
    /// Parse a single modifier name (case-insensitive)
    pub(crate) fn parse_single(s: &str) -> Option<Modifiers> {
        match s.to_lowercase().as_str() {
            // Compound (either side)
            "cmd" | "command" | "meta" | "super" | "win" | "windows" => Some(Modifiers::CMD),
            "shift" => Some(Modifiers::SHIFT),
            "ctrl" | "control" => Some(Modifiers::CTRL),
            "opt" | "option" | "alt" => Some(Modifiers::OPT),
            "fn" | "function" => Some(Modifiers::FN),

            // Left-specific
            "cmdleft" | "cmd_left" | "lcmd" | "commandleft" | "command_left" | "lcommand"
            | "superleft" | "super_left" | "winleft" | "win_left" | "windowsleft"
            | "windows_left" | "metaleft" | "meta_left" => Some(Modifiers::CMD_LEFT),
            "shiftleft" | "shift_left" | "lshift" => Some(Modifiers::SHIFT_LEFT),
            "ctrlleft" | "ctrl_left" | "lctrl" | "controlleft" | "control_left" | "lcontrol" => {
                Some(Modifiers::CTRL_LEFT)
            }
            "optleft" | "opt_left" | "lopt" | "optionleft" | "option_left" | "loption"
            | "altleft" | "alt_left" | "lalt" => Some(Modifiers::OPT_LEFT),

            // Right-specific
            "cmdright" | "cmd_right" | "rcmd" | "commandright" | "command_right" | "rcommand"
            | "superright" | "super_right" | "winright" | "win_right" | "windowsright"
            | "windows_right" | "metaright" | "meta_right" => Some(Modifiers::CMD_RIGHT),
            "shiftright" | "shift_right" | "rshift" => Some(Modifiers::SHIFT_RIGHT),
            "ctrlright" | "ctrl_right" | "rctrl" | "controlright" | "control_right"
            | "rcontrol" => Some(Modifiers::CTRL_RIGHT),
            "optright" | "opt_right" | "ropt" | "optionright" | "option_right" | "roption"
            | "altright" | "alt_right" | "ralt" | "altgr" => Some(Modifiers::OPT_RIGHT),

            _ => None,
        }
    }

    /// Check whether `self` (as a hotkey pattern) matches `event` (the actual modifier state).
    ///
    /// For each modifier group (Cmd, Shift, Ctrl, Opt):
    /// - Hotkey has both bits (compound): event must have at least one bit from the group
    /// - Hotkey has a specific side: event must have that specific side (extra same-group bits OK)
    /// - Hotkey has neither: event must not have either bit from the group
    ///
    /// FN is matched exactly.
    pub fn matches(self, event: Modifiers) -> bool {
        for &(left, right, _compound) in &GROUPS {
            let hotkey_has_left = self.contains(left);
            let hotkey_has_right = self.contains(right);
            let event_has_left = event.contains(left);
            let event_has_right = event.contains(right);
            let event_has_any = event_has_left || event_has_right;

            if hotkey_has_left && hotkey_has_right {
                // Compound: event must have at least one
                if !event_has_any {
                    return false;
                }
            } else if hotkey_has_left {
                // Specific left: event must have left
                if !event_has_left {
                    return false;
                }
            } else if hotkey_has_right {
                // Specific right: event must have right
                if !event_has_right {
                    return false;
                }
            } else {
                // Hotkey doesn't use this group: event must not have it
                if event_has_any {
                    return false;
                }
            }
        }

        // FN: exact match
        self.contains(Modifiers::FN) == event.contains(Modifiers::FN)
    }
}

impl FromStr for Modifiers {
    type Err = Error;

    /// Parse modifiers from a string like "Cmd+Shift" or "Ctrl+Alt"
    fn from_str(s: &str) -> Result<Self> {
        let s = s.trim();
        if s.is_empty() {
            return Ok(Modifiers::empty());
        }

        let mut modifiers = Modifiers::empty();
        for part in s.split('+') {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            match Modifiers::parse_single(part) {
                Some(m) => modifiers |= m,
                None => return Err(Error::UnknownModifier(part.to_string())),
            }
        }
        Ok(modifiers)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_single_modifiers() {
        assert_eq!("Cmd".parse::<Modifiers>().unwrap(), Modifiers::CMD);
        assert_eq!("command".parse::<Modifiers>().unwrap(), Modifiers::CMD);
        assert_eq!("meta".parse::<Modifiers>().unwrap(), Modifiers::CMD);
        assert_eq!("super".parse::<Modifiers>().unwrap(), Modifiers::CMD);
        assert_eq!("win".parse::<Modifiers>().unwrap(), Modifiers::CMD);

        assert_eq!("Shift".parse::<Modifiers>().unwrap(), Modifiers::SHIFT);
        assert_eq!("SHIFT".parse::<Modifiers>().unwrap(), Modifiers::SHIFT);

        assert_eq!("Ctrl".parse::<Modifiers>().unwrap(), Modifiers::CTRL);
        assert_eq!("control".parse::<Modifiers>().unwrap(), Modifiers::CTRL);

        assert_eq!("Opt".parse::<Modifiers>().unwrap(), Modifiers::OPT);
        assert_eq!("option".parse::<Modifiers>().unwrap(), Modifiers::OPT);
        assert_eq!("alt".parse::<Modifiers>().unwrap(), Modifiers::OPT);

        assert_eq!("Fn".parse::<Modifiers>().unwrap(), Modifiers::FN);
        assert_eq!("function".parse::<Modifiers>().unwrap(), Modifiers::FN);
    }

    #[test]
    fn parse_side_specific_modifiers() {
        assert_eq!("CmdLeft".parse::<Modifiers>().unwrap(), Modifiers::CMD_LEFT);
        assert_eq!("LCmd".parse::<Modifiers>().unwrap(), Modifiers::CMD_LEFT);
        assert_eq!(
            "CmdRight".parse::<Modifiers>().unwrap(),
            Modifiers::CMD_RIGHT
        );
        assert_eq!("RCmd".parse::<Modifiers>().unwrap(), Modifiers::CMD_RIGHT);

        assert_eq!(
            "ShiftLeft".parse::<Modifiers>().unwrap(),
            Modifiers::SHIFT_LEFT
        );
        assert_eq!(
            "ShiftRight".parse::<Modifiers>().unwrap(),
            Modifiers::SHIFT_RIGHT
        );

        assert_eq!(
            "CtrlLeft".parse::<Modifiers>().unwrap(),
            Modifiers::CTRL_LEFT
        );
        assert_eq!(
            "CtrlRight".parse::<Modifiers>().unwrap(),
            Modifiers::CTRL_RIGHT
        );

        assert_eq!("OptLeft".parse::<Modifiers>().unwrap(), Modifiers::OPT_LEFT);
        assert_eq!(
            "AltRight".parse::<Modifiers>().unwrap(),
            Modifiers::OPT_RIGHT
        );
        assert_eq!("AltGr".parse::<Modifiers>().unwrap(), Modifiers::OPT_RIGHT);
    }

    #[test]
    fn parse_combined_modifiers() {
        assert_eq!(
            "Cmd+Shift".parse::<Modifiers>().unwrap(),
            Modifiers::CMD | Modifiers::SHIFT
        );
        assert_eq!(
            "Ctrl+Alt+Shift".parse::<Modifiers>().unwrap(),
            Modifiers::CTRL | Modifiers::OPT | Modifiers::SHIFT
        );
    }

    #[test]
    fn parse_empty_modifiers() {
        assert_eq!("".parse::<Modifiers>().unwrap(), Modifiers::empty());
        assert_eq!("  ".parse::<Modifiers>().unwrap(), Modifiers::empty());
    }

    #[test]
    fn parse_unknown_modifier_fails() {
        assert!("Unknown".parse::<Modifiers>().is_err());
        assert!("Cmd+Unknown".parse::<Modifiers>().is_err());
    }

    #[test]
    fn modifiers_display() {
        assert_eq!(format!("{}", Modifiers::CMD), "Cmd");
        assert_eq!(format!("{}", Modifiers::SHIFT), "Shift");
        assert_eq!(
            format!("{}", Modifiers::CMD | Modifiers::SHIFT),
            "Shift+Cmd"
        );
    }

    #[test]
    fn modifiers_display_side_specific() {
        assert_eq!(format!("{}", Modifiers::CMD_LEFT), "CmdLeft");
        assert_eq!(format!("{}", Modifiers::CMD_RIGHT), "CmdRight");
        assert_eq!(format!("{}", Modifiers::SHIFT_LEFT), "ShiftLeft");
        assert_eq!(format!("{}", Modifiers::CTRL_RIGHT), "CtrlRight");
        assert_eq!(format!("{}", Modifiers::OPT_LEFT), "OptLeft");
    }

    #[test]
    fn matches_compound_hotkey() {
        // Compound "Cmd" matches either side
        let hotkey = Modifiers::CMD;
        assert!(hotkey.matches(Modifiers::CMD_LEFT));
        assert!(hotkey.matches(Modifiers::CMD_RIGHT));
        assert!(hotkey.matches(Modifiers::CMD_LEFT | Modifiers::CMD_RIGHT));
        assert!(!hotkey.matches(Modifiers::empty()));
        assert!(!hotkey.matches(Modifiers::SHIFT_LEFT));
    }

    #[test]
    fn matches_side_specific_hotkey() {
        // Specific "CmdLeft" requires left
        let hotkey = Modifiers::CMD_LEFT;
        assert!(hotkey.matches(Modifiers::CMD_LEFT));
        assert!(!hotkey.matches(Modifiers::CMD_RIGHT));
        // Both sides pressed: left is still present, so it matches
        assert!(hotkey.matches(Modifiers::CMD_LEFT | Modifiers::CMD_RIGHT));
        assert!(!hotkey.matches(Modifiers::empty()));
    }

    #[test]
    fn matches_rejects_extra_groups() {
        // Hotkey is just Cmd, event has Cmd+Shift — should fail (extra group)
        let hotkey = Modifiers::CMD;
        assert!(!hotkey.matches(Modifiers::CMD_LEFT | Modifiers::SHIFT_LEFT));

        // Hotkey is CmdLeft+ShiftLeft, event is CmdLeft+ShiftLeft — OK
        let hotkey = Modifiers::CMD_LEFT | Modifiers::SHIFT_LEFT;
        assert!(hotkey.matches(Modifiers::CMD_LEFT | Modifiers::SHIFT_LEFT));
    }

    #[test]
    fn matches_fn_exact() {
        let hotkey = Modifiers::CMD | Modifiers::FN;
        assert!(hotkey.matches(Modifiers::CMD_LEFT | Modifiers::FN));
        assert!(!hotkey.matches(Modifiers::CMD_LEFT)); // missing FN

        let hotkey = Modifiers::CMD;
        assert!(!hotkey.matches(Modifiers::CMD_LEFT | Modifiers::FN)); // extra FN
    }

    #[test]
    fn matches_empty() {
        let hotkey = Modifiers::empty();
        assert!(hotkey.matches(Modifiers::empty()));
        assert!(!hotkey.matches(Modifiers::CMD_LEFT));
    }

    #[test]
    fn compound_equals_both_sides() {
        assert_eq!(Modifiers::CMD, Modifiers::CMD_LEFT | Modifiers::CMD_RIGHT);
        assert_eq!(
            Modifiers::SHIFT,
            Modifiers::SHIFT_LEFT | Modifiers::SHIFT_RIGHT
        );
        assert_eq!(
            Modifiers::CTRL,
            Modifiers::CTRL_LEFT | Modifiers::CTRL_RIGHT
        );
        assert_eq!(Modifiers::OPT, Modifiers::OPT_LEFT | Modifiers::OPT_RIGHT);
    }
}

/**
 * Keyboard utility functions for handling keyboard events
 */

export type OSType = "macos" | "windows" | "linux" | "unknown";

/**
 * Extract a consistent key name from a KeyboardEvent
 * This function provides cross-platform keyboard event handling
 * and returns key names appropriate for the target operating system
 */
export const getKeyName = (
  e: KeyboardEvent,
  osType: OSType = "unknown",
): string => {
  const normalizeModifier = (name: string): string => {
    switch (name) {
      case "Control":
        return "ctrl";
      case "Alt":
        return osType === "macos" ? "option" : "alt";
      case "Meta":
      case "OS":
        return osType === "macos" ? "command" : osType === "windows" ? "win" : "super";
      case "Shift":
        return "shift";
      default:
        return name.toLowerCase();
    }
  };

  const key = e.key;

  if (key && key !== "Unidentified" && key !== "Dead") {
    const specialMap: Record<string, string> = {
      Backspace: "backspace",
      Tab: "tab",
      Enter: "enter",
      Return: "enter",
      Escape: "esc",
      " ": "space",
      Spacebar: "space",
      CapsLock: "caps lock",
      ContextMenu: "menu",
      Delete: "delete",
      End: "end",
      Home: "home",
      Insert: "insert",
      PageDown: "page down",
      PageUp: "page up",
      PrintScreen: "print screen",
      ScrollLock: "scroll lock",
      Pause: "pause",
      NumLock: "num lock",
      ArrowDown: "down",
      ArrowLeft: "left",
      ArrowRight: "right",
      ArrowUp: "up",
    };

    if (specialMap[key]) {
      return specialMap[key];
    }

    if (/^F\d{1,2}$/i.test(key)) {
      return key.toLowerCase();
    }

    if (key === key.toUpperCase() && key.length === 1 && key !== key.toLowerCase()) {
      return key.toLowerCase();
    }

    if (key.length === 1) {
      return key.toLowerCase();
    }

    return normalizeModifier(key);
  }

  if (e.code) {
    const code = e.code;

    if (/^F\d{1,2}$/i.test(code)) {
      return code.toLowerCase();
    }

    if (code.startsWith("Numpad")) {
      const suffix = code.slice("Numpad".length);
      const map: Record<string, string> = {
        Add: "numpad +",
        Subtract: "numpad -",
        Multiply: "numpad *",
        Divide: "numpad /",
        Decimal: "numpad .",
      };
      if (map[suffix]) {
        return map[suffix];
      }
      if (/^\d$/.test(suffix)) {
        return `numpad ${suffix}`;
      }
    }

    const modifierMap: Record<string, string> = {
      ShiftLeft: "shift",
      ShiftRight: "shift",
      ControlLeft: "ctrl",
      ControlRight: "ctrl",
      AltLeft: osType === "macos" ? "option" : "alt",
      AltRight: osType === "macos" ? "option" : "alt",
      MetaLeft: osType === "macos" ? "command" : osType === "windows" ? "win" : "super",
      MetaRight: osType === "macos" ? "command" : osType === "windows" ? "win" : "super",
      OSLeft: osType === "macos" ? "command" : osType === "windows" ? "win" : "super",
      OSRight: osType === "macos" ? "command" : osType === "windows" ? "win" : "super",
    };

    if (modifierMap[code]) {
      return modifierMap[code];
    }

    const punctuationMap: Record<string, string> = {
      Semicolon: ";",
      Equal: "=",
      Comma: ",",
      Minus: "-",
      Period: ".",
      Slash: "/",
      Backquote: "`",
      BracketLeft: "[",
      Backslash: "\\",
      BracketRight: "]",
      Quote: "'",
    };

    if (punctuationMap[code]) {
      return punctuationMap[code];
    }

    if (code.startsWith("Digit")) {
      return code.replace("Digit", "");
    }

    return code.toLowerCase().replace(/([a-z])([A-Z])/g, "$1 $2");
  }

  return `unknown-${e.keyCode || e.which || 0}`;
};

/**
 * Get display-friendly key combination string for the current OS
 * Returns basic plus-separated format with correct platform key names
 */
export const formatKeyCombination = (
  combination: string,
  osType: OSType,
): string => {
  const formatToken = (token: string): string => {
    const trimmed = token.trim();
    const lower = trimmed.toLowerCase();
    const modifierMap: Record<string, string> = {
      ctrl: "Ctrl",
      control: "Ctrl",
      shift: "Shift",
      alt: "Alt",
      option: osType === "macos" ? "Option" : "Alt",
      command: osType === "macos" ? "Command" : "Super",
      meta: osType === "macos" ? "Command" : osType === "windows" ? "Win" : "Super",
      super: "Super",
      win: "Win",
    };

    if (modifierMap[lower]) {
      return modifierMap[lower];
    }

    if (lower.startsWith("unicode:")) {
      const value = trimmed.slice("unicode:".length).replace(/^'|'$/g, "");
      return value.length === 1 ? value.toUpperCase() : value;
    }

    if (lower.startsWith("keycode:")) {
      return `Keycode ${trimmed.slice("keycode:".length)}`;
    }

    if (lower.length === 1) {
      return lower.toUpperCase();
    }

    return trimmed
      .split(" ")
      .map((part) => part.charAt(0).toUpperCase() + part.slice(1))
      .join(" ");
  };

  return combination
    .split("+")
    .map((token) => token.trim())
    .filter(Boolean)
    .map(formatToken)
    .join("+");
};

/**
 * Normalize modifier keys to handle left/right variants
 */
export const normalizeKey = (key: string): string => {
  // Handle left/right variants of modifier keys
  if (key.startsWith("left ") || key.startsWith("right ")) {
    const parts = key.split(" ");
    if (parts.length === 2) {
      // Return just the modifier name without left/right prefix
      return parts[1];
    }
  }
  return key;
};

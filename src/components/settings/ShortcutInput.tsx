import React from "react";
import { useSettings } from "../../hooks/useSettings";
import { GlobalShortcutInput } from "./GlobalShortcutInput";
import { HandyKeysShortcutInput } from "./HandyKeysShortcutInput";

interface ShortcutInputProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
  shortcutId: string;
  disabled?: boolean;
}

/**
 * Wrapper component that selects the appropriate shortcut input implementation
 * based on the keyboard_implementation setting.
 *
 * - "handy_keys" (default): Uses HandyKeysShortcutInput with backend key events
 * - "tauri": Uses GlobalShortcutInput with JS keyboard events (needed for key
 *   remappers like Hyperkey/Karabiner)
 */
export const ShortcutInput: React.FC<ShortcutInputProps> = (props) => {
  const { getSetting } = useSettings();
  const keyboardImplementation = getSetting("keyboard_implementation");

  // Default to Handy Keys implementation if not set
  if (keyboardImplementation === "tauri") {
    return <GlobalShortcutInput {...props} />;
  }

  return <HandyKeysShortcutInput {...props} />;
};

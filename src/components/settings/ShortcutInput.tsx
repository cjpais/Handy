import React from "react";
import { HandyKeysShortcutInput } from "./HandyKeysShortcutInput";

interface ShortcutInputProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
  shortcutId: string;
  disabled?: boolean;
}

/**
 * Shortcut recording uses native backend keyboard events for layout-aware
 * key capture across Windows, Linux, and macOS.
 */
export const ShortcutInput: React.FC<ShortcutInputProps> = (props) => {
  return <HandyKeysShortcutInput {...props} />;
};

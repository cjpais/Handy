import { useEffect, useRef } from "react";
import { getKeyName, type OSType } from "@/lib/utils/keyboard";

export type PressKind = "hold" | "tap";

interface ShortcutPressHandlers {
  onDown?: () => void;
  /** A press held at least the threshold is a hold; a shorter one a tap. */
  onUp?: (kind: PressKind) => void;
}

// Binding tokens and getKeyName's names for the same key.
const SAME_KEY: Record<string, string> = {
  control: "ctrl",
  alt: "option",
  meta: "command",
  cmd: "command",
  win: "super",
};

const normalize = (key: string) => {
  const base = key
    .trim()
    .toLowerCase()
    .replace(/_(left|right)$/, "");
  return SAME_KEY[base] ?? base;
};

/**
 * Listens for a shortcut in this window's own key events: onDown when every
 * key of the binding is down, onUp with whether it was held or tapped (Handy's
 * hold-or-toggle rule). For screens that react to the shortcut while the
 * global bindings are suspended, so pressing it doesn't start a recording.
 */
export const useShortcutPress = (
  binding: string | undefined,
  osType: OSType,
  holdThresholdMs: number,
  handlers: ShortcutPressHandlers,
  enabled = true,
) => {
  const handlersRef = useRef(handlers);
  handlersRef.current = handlers;

  useEffect(() => {
    if (!binding || !enabled) return;
    const want = binding.split("+").map(normalize).filter(Boolean);
    const down = new Set<string>();
    let pressedAt: number | null = null;

    const name = (e: KeyboardEvent) => normalize(getKeyName(e, osType));
    const onKeyDown = (e: KeyboardEvent) => {
      down.add(name(e));
      if (!want.every((k) => down.has(k))) return;
      e.preventDefault();
      if (e.repeat || pressedAt != null) return;
      pressedAt = performance.now();
      handlersRef.current.onDown?.();
    };
    const onKeyUp = (e: KeyboardEvent) => {
      const key = name(e);
      down.delete(key);
      if (pressedAt == null || !want.includes(key)) return;
      e.preventDefault();
      const held = performance.now() - pressedAt;
      pressedAt = null;
      handlersRef.current.onUp?.(held >= holdThresholdMs ? "hold" : "tap");
    };
    // Keys released while the window was in the background never come up.
    const onBlur = () => {
      down.clear();
      if (pressedAt != null) {
        pressedAt = null;
        handlersRef.current.onUp?.("hold");
      }
    };
    window.addEventListener("keydown", onKeyDown, true);
    window.addEventListener("keyup", onKeyUp, true);
    window.addEventListener("blur", onBlur);
    return () => {
      window.removeEventListener("keydown", onKeyDown, true);
      window.removeEventListener("keyup", onKeyUp, true);
      window.removeEventListener("blur", onBlur);
    };
  }, [binding, osType, holdThresholdMs, enabled]);
};

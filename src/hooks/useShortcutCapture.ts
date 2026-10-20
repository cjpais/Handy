import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { commands } from "@/bindings";
import { useSettings } from "@/hooks/useSettings";
import { getKeyName, normalizeKey, type OSType } from "@/lib/utils/keyboard";

interface HandyKeysEvent {
  modifiers: string[];
  key: string | null;
  is_key_down: boolean;
  hotkey_string: string;
}

const MODIFIERS = [
  "ctrl",
  "control",
  "shift",
  "alt",
  "option",
  "meta",
  "command",
  "cmd",
  "super",
  "win",
  "windows",
];

/**
 * Records a new combination for a binding the way Settings does: with the
 * handy-keys backend's key events where that's the keyboard implementation,
 * else from this window's own key events. `partial` is what's held so far.
 * Calls onDone once the binding is saved or capture is cancelled. `inert`
 * (onboarding's preview) records from this window and saves nothing.
 */
export const useShortcutCapture = (
  bindingId: string,
  osType: OSType,
  onDone?: () => void,
  inert = false,
) => {
  const { t } = useTranslation();
  const { getSetting, updateBinding } = useSettings();
  const handyKeys =
    !inert && getSetting("keyboard_implementation") === "handy_keys";
  const [capturing, setCapturing] = useState(false);
  const [partial, setPartial] = useState("");
  const onDoneRef = useRef(onDone);
  onDoneRef.current = onDone;

  const finish = useCallback(async () => {
    if (handyKeys) await commands.stopHandyKeysRecording().catch(console.error);
    else if (!inert) await commands.resumeAllBindings().catch(console.error);
    setCapturing(false);
    setPartial("");
    onDoneRef.current?.();
  }, [handyKeys, inert]);

  const commit = useCallback(
    async (binding: string) => {
      try {
        if (!inert) await updateBinding(bindingId, binding);
      } catch (error) {
        console.error("Failed to change binding:", error);
        toast.error(
          t("settings.general.shortcut.errors.set", { error: String(error) }),
        );
      }
      await finish();
    },
    [bindingId, finish, inert, t, updateBinding],
  );

  const start = useCallback(async () => {
    if (handyKeys) {
      const result = await commands.startHandyKeysRecording(bindingId);
      if (result.status === "error") {
        toast.error(
          t("settings.general.shortcut.errors.set", { error: result.error }),
        );
        return;
      }
    } else if (!inert) {
      await commands.suspendAllBindings().catch(console.error);
    }
    setPartial("");
    setCapturing(true);
  }, [bindingId, handyKeys, inert, t]);

  const cancel = useCallback(() => {
    if (capturing) finish();
  }, [capturing, finish]);

  // Escape cancels.
  useEffect(() => {
    if (!capturing) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      e.preventDefault();
      e.stopPropagation();
      finish();
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [capturing, finish]);

  // handy-keys: the backend reports keys while recording.
  useEffect(() => {
    if (!capturing || !handyKeys) return;
    let keyed = "";
    let modifierOnly = "";
    let stopped = false;
    const pending = listen<HandyKeysEvent>("handy-keys-event", (event) => {
      if (stopped) return;
      const { hotkey_string, is_key_down, key, modifiers } = event.payload;
      if (is_key_down && hotkey_string) {
        if (key) keyed = hotkey_string;
        else modifierOnly = hotkey_string;
        setPartial(hotkey_string);
      } else if (!is_key_down && key) {
        // The key came up: commit the keyed combination (never just the
        // modifiers, which is how bindings used to get overwritten).
        const binding = keyed || hotkey_string;
        if (binding) {
          stopped = true;
          commit(binding);
        }
      } else if (!is_key_down && !key && !modifiers.length && !keyed) {
        if (modifierOnly) {
          stopped = true;
          commit(modifierOnly);
        }
      }
    });
    return () => {
      stopped = true;
      pending.then((unlisten) => unlisten());
    };
  }, [capturing, handyKeys, commit]);

  // Tauri shortcuts: this window's own key events.
  useEffect(() => {
    if (!capturing || handyKeys) return;
    const pressed = new Set<string>();
    const recorded: string[] = [];
    const onDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") return;
      e.preventDefault();
      const key = normalizeKey(getKeyName(e, osType));
      pressed.add(key);
      if (!recorded.includes(key)) recorded.push(key);
      setPartial(recorded.join("+"));
    };
    const onUp = (e: KeyboardEvent) => {
      e.preventDefault();
      pressed.delete(normalizeKey(getKeyName(e, osType)));
      if (pressed.size || !recorded.length) return;
      const sorted = [...recorded].sort(
        (a, b) => +!MODIFIERS.includes(a) - +!MODIFIERS.includes(b),
      );
      commit(sorted.join("+"));
    };
    window.addEventListener("keydown", onDown);
    window.addEventListener("keyup", onUp);
    return () => {
      window.removeEventListener("keydown", onDown);
      window.removeEventListener("keyup", onUp);
    };
  }, [capturing, handyKeys, commit, osType]);

  return { capturing, partial, start, cancel };
};

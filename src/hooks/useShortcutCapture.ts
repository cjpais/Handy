import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { commands } from "@/bindings";
import { SECURE_INPUT_HELP_URL } from "@/components/SecureInputWarning";
import { useSettings } from "@/hooks/useSettings";
import {
  getKeyName,
  MODIFIERS,
  normalizeKey,
  type OSType,
} from "@/lib/utils/keyboard";

interface HandyKeysEvent {
  modifiers: string[];
  key: string | null;
  is_key_down: boolean;
  hotkey_string: string;
}

/**
 * Records a new binding via handy-keys events, or this window's key events
 * with all bindings suspended. `inert` records without saving;
 * `escapeCancels` makes Escape exit instead of recording.
 */
export const useShortcutCapture = (
  bindingId: string,
  osType: OSType,
  {
    onDone,
    inert = false,
    escapeCancels = false,
  }: { onDone?: () => void; inert?: boolean; escapeCancels?: boolean } = {},
) => {
  const { t } = useTranslation();
  const { getSetting, updateBinding } = useSettings();
  const handyKeys =
    !inert && getSetting("keyboard_implementation") === "handy_keys";
  const [capturing, setCapturing] = useState(false);
  const [partial, setPartial] = useState("");
  const onDoneRef = useRef(onDone);
  onDoneRef.current = onDone;

  // What the capture holds in the backend, released on end or unmount.
  const held = useRef<"handyKeys" | "bindings" | null>(null);
  const release = useCallback(async () => {
    const was = held.current;
    held.current = null;
    if (was === "handyKeys")
      await commands.stopHandyKeysRecording().catch(console.error);
    else if (was === "bindings")
      await commands.resumeAllBindings().catch(console.error);
  }, []);
  useEffect(
    () => () => {
      release();
    },
    [release],
  );

  const finish = useCallback(async () => {
    setCapturing(false);
    setPartial("");
    await release();
    onDoneRef.current?.();
  }, [release]);

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

  /** Resolves to whether capture began. */
  const start = useCallback(async () => {
    if (held.current) return false;
    if (handyKeys) {
      // Refused while macOS Secure Input is on (no key events would arrive);
      // the backend also shows the warning banner the toast points at.
      const result = await commands
        .startHandyKeysRecording(bindingId)
        .catch((error) => ({ status: "error" as const, error: String(error) }));
      if (result.status === "error") {
        if (String(result.error).includes("secure-input-active")) {
          toast.error(t("secureInput.recorderBlocked"), {
            action: {
              label: t("secureInput.learnMore"),
              onClick: () => openUrl(SECURE_INPUT_HELP_URL),
            },
          });
        } else {
          toast.error(
            t("settings.general.shortcut.errors.set", { error: result.error }),
          );
        }
        return false;
      }
      held.current = "handyKeys";
    } else if (!inert) {
      await commands.suspendAllBindings().catch(console.error);
      held.current = "bindings";
    }
    setPartial("");
    setCapturing(true);
    return true;
  }, [bindingId, handyKeys, inert, t]);

  const cancel = useCallback(() => {
    if (capturing) finish();
  }, [capturing, finish]);

  useEffect(() => {
    if (!capturing || !escapeCancels) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      e.preventDefault();
      e.stopPropagation();
      finish();
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [capturing, escapeCancels, finish]);

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
        // Commit the keyed combo, never bare modifiers (that overwrote bindings).
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
      if (escapeCancels && e.key === "Escape") return;
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
  }, [capturing, handyKeys, commit, escapeCancels, osType]);

  return { capturing, partial, start, cancel };
};

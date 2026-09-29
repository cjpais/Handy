import type { AppSettings as Settings, ShortcutOverrides } from "@/bindings";
import { useSettings } from "./useSettings";

/** Global settings a transcribe shortcut can override for itself. */
export type ShortcutSettingKey =
  | "shortcut_activation"
  | "paste_method"
  | "clipboard_handling"
  | "auto_submit"
  | "auto_submit_key";

const OVERRIDE_FIELD: {
  [K in ShortcutSettingKey]: keyof ShortcutOverrides;
} = {
  shortcut_activation: "activation",
  paste_method: "paste_method",
  clipboard_handling: "clipboard_handling",
  auto_submit: "auto_submit",
  auto_submit_key: "auto_submit_key",
};

/**
 * The value a shortcut runs with: its own override, else the global setting.
 * Mirrors `AppSettings::for_binding` in the backend.
 */
export const effectiveShortcutSetting = <K extends ShortcutSettingKey>(
  settings: Settings | null,
  bindingId: string | undefined,
  key: K,
): Settings[K] | undefined => {
  const override = bindingId
    ? settings?.bindings?.[bindingId]?.overrides?.[OVERRIDE_FIELD[key]]
    : undefined;
  return (override ?? settings?.[key]) as Settings[K] | undefined;
};

/**
 * Read and write a setting either globally or, with `bindingId`, for one
 * transcribe shortcut only.
 */
export const useShortcutSetting = <K extends ShortcutSettingKey>(
  bindingId: string | undefined,
  key: K,
) => {
  const { settings, updateSetting, updateBindingOverrides, isUpdating } =
    useSettings();

  return {
    value: effectiveShortcutSetting(settings, bindingId, key),
    update: (value: Settings[K]) =>
      bindingId
        ? updateBindingOverrides(bindingId, { [OVERRIDE_FIELD[key]]: value })
        : updateSetting(key, value),
    isUpdating: bindingId
      ? isUpdating(`binding_overrides_${bindingId}`)
      : isUpdating(key),
  };
};

import { platform } from "@tauri-apps/plugin-os";
import React from "react";
import { useTranslation } from "react-i18next";
import { ToggleSwitch } from "../ui/ToggleSwitch";
import { useSettings } from "../../hooks/useSettings";

interface ShortcutPassthroughProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const ShortcutPassthrough: React.FC<ShortcutPassthroughProps> =
  React.memo(({ descriptionMode = "tooltip", grouped = false }) => {
    const { t } = useTranslation();
    const { getSetting, updateSetting, isUpdating } = useSettings();

    const enabled = getSetting("shortcut_passthrough") ?? false;

    if (
      platform() !== "windows" ||
      getSetting("keyboard_implementation") !== "handy_keys"
    )
      return null;

    return (
      <ToggleSwitch
        checked={enabled}
        onChange={(enabled) => updateSetting("shortcut_passthrough", enabled)}
        isUpdating={isUpdating("shortcut_passthrough")}
        label={t("settings.advanced.shortcutPassthrough.label")}
        description={t("settings.advanced.shortcutPassthrough.description")}
        descriptionMode={descriptionMode}
        grouped={grouped}
      />
    );
  });

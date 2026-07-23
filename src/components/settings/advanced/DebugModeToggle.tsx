import React from "react";
import { useTranslation } from "react-i18next";
import { ToggleSwitch } from "../../ui/ToggleSwitch";
import { useSettings } from "../../../hooks/useSettings";

interface DebugModeToggleProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const DebugModeToggle: React.FC<DebugModeToggleProps> = React.memo(
  ({ descriptionMode = "tooltip", grouped = false }) => {
    const { t } = useTranslation();
    const { getSetting, updateSetting, isUpdating } = useSettings();

    const enabled = getSetting("debug_mode") ?? false;

    return (
      <ToggleSwitch
        checked={enabled}
        onChange={(enabled) => updateSetting("debug_mode", enabled)}
        isUpdating={isUpdating("debug_mode")}
        label={t("settings.advanced.debugMode.label")}
        description={t("settings.advanced.debugMode.description")}
        descriptionMode={descriptionMode}
        grouped={grouped}
      />
    );
  },
);

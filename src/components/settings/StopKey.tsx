import React from "react";
import { useTranslation } from "react-i18next";
import { ToggleSwitch } from "../ui/ToggleSwitch";
import { useSettings } from "../../hooks/useSettings";

interface StopKeyProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const StopKey: React.FC<StopKeyProps> = React.memo(
  ({ descriptionMode = "tooltip", grouped = false }) => {
    const { t } = useTranslation();
    const { getSetting, updateSetting, isUpdating } = useSettings();

    const stopKeyEnabled = getSetting("stop_key_enabled") || false;

    return (
      <ToggleSwitch
        checked={stopKeyEnabled}
        onChange={(enabled) => updateSetting("stop_key_enabled", enabled)}
        isUpdating={isUpdating("stop_key_enabled")}
        label={t("settings.general.stopKey.label")}
        description={t("settings.general.stopKey.description")}
        descriptionMode={descriptionMode}
        grouped={grouped}
      />
    );
  },
);

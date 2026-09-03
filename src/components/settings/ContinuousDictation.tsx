import React from "react";
import { useTranslation } from "react-i18next";
import { ToggleSwitch } from "../ui/ToggleSwitch";
import { useSettings } from "../../hooks/useSettings";

interface ContinuousDictationProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const ContinuousDictation: React.FC<ContinuousDictationProps> = React.memo(
  ({ descriptionMode = "tooltip", grouped = false }) => {
    const { t } = useTranslation();
    const { getSetting, updateSetting, isUpdating } = useSettings();

    const continuousDictationEnabled = getSetting("continuous_dictation_enabled") || false;

    return (
      <ToggleSwitch
        checked={continuousDictationEnabled}
        onChange={(enabled) => updateSetting("continuous_dictation_enabled", enabled)}
        isUpdating={isUpdating("continuous_dictation_enabled")}
        label={t("settings.debug.continuousDictation.label")}
        description={t("settings.debug.continuousDictation.description")}
        descriptionMode={descriptionMode}
        grouped={grouped}
      />
    );
  },
);

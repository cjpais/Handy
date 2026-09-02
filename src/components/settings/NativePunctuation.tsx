import React from "react";
import { useTranslation } from "react-i18next";
import { ToggleSwitch } from "../ui/ToggleSwitch";
import { useSettings } from "../../hooks/useSettings";

interface NativePunctuationProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const NativePunctuation: React.FC<NativePunctuationProps> = React.memo(
  ({ descriptionMode = "tooltip", grouped = false }) => {
    const { t } = useTranslation();
    const { getSetting, updateSetting, isUpdating } = useSettings();
    const enabled = getSetting("native_itn") ?? true;

    return (
      <ToggleSwitch
        checked={enabled}
        onChange={(value) => updateSetting("native_itn", value)}
        isUpdating={isUpdating("native_itn")}
        label={t("settings.modelSettings.nativePunctuation.label")}
        description={t("settings.modelSettings.nativePunctuation.description")}
        descriptionMode={descriptionMode}
        grouped={grouped}
      />
    );
  },
);

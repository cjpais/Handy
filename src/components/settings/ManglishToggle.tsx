import React from "react";
import { useTranslation } from "react-i18next";
import { ToggleSwitch } from "../ui/ToggleSwitch";
import { useSettings } from "../../hooks/useSettings";

interface ManglishToggleProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const ManglishToggle: React.FC<ManglishToggleProps> = React.memo(
  ({ descriptionMode = "tooltip", grouped = false }) => {
    const { t } = useTranslation();
    const { getSetting, updateSetting, isUpdating } = useSettings();

    const enabled = getSetting("manglish_output") || false;

    return (
      <ToggleSwitch
        checked={enabled}
        onChange={(enabled) => updateSetting("manglish_output", enabled)}
        isUpdating={isUpdating("manglish_output")}
        label={t("settings.general.manglishToggle.label")}
        description={t("settings.general.manglishToggle.description")}
        descriptionMode={descriptionMode}
        grouped={grouped}
      />
    );
  },
);

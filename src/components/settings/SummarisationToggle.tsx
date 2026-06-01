import React from "react";
import { useTranslation } from "react-i18next";
import { ToggleSwitch } from "../ui/ToggleSwitch";
import { useSettings } from "../../hooks/useSettings";

interface SummarisationToggleProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const SummarisationToggle: React.FC<SummarisationToggleProps> =
  React.memo(({ descriptionMode = "tooltip", grouped = false }) => {
    const { t } = useTranslation();
    const { getSetting, updateSetting, isUpdating } = useSettings();

    const enabled = getSetting("summarize_enabled") || false;

    return (
      <ToggleSwitch
        checked={enabled}
        onChange={(value) => updateSetting("summarize_enabled", value)}
        isUpdating={isUpdating("summarize_enabled")}
        label={t("settings.summarisation.toggle.label")}
        description={t("settings.summarisation.toggle.description")}
        descriptionMode={descriptionMode}
        grouped={grouped}
      />
    );
  });

SummarisationToggle.displayName = "SummarisationToggle";

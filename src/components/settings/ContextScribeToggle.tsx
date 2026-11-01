import React from "react";
import { useTranslation } from "react-i18next";
import { ToggleSwitch } from "../ui/ToggleSwitch";
import { useSettings } from "../../hooks/useSettings";

interface ContextScribeToggleProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const ContextScribeToggle: React.FC<ContextScribeToggleProps> =
  React.memo(({ descriptionMode = "tooltip", grouped = false }) => {
    const { t } = useTranslation();
    const { getSetting, updateSetting, isUpdating } = useSettings();

    const enabled = getSetting("context_scribe_enabled") || false;

    return (
      <ToggleSwitch
        checked={enabled}
        onChange={(enabled) => updateSetting("context_scribe_enabled", enabled)}
        isUpdating={isUpdating("context_scribe_enabled")}
        label={t("settings.debug.contextScribeToggle.label")}
        description={t("settings.debug.contextScribeToggle.description")}
        descriptionMode={descriptionMode}
        grouped={grouped}
      />
    );
  });

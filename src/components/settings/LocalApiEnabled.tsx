import React from "react";
import { useTranslation } from "react-i18next";
import { ToggleSwitch } from "../ui/ToggleSwitch";
import { useSettings } from "../../hooks/useSettings";

interface LocalApiEnabledProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const LocalApiEnabled: React.FC<LocalApiEnabledProps> = React.memo(
  ({ descriptionMode = "tooltip", grouped = false }) => {
    const { t } = useTranslation();
    const { getSetting, updateSetting, isUpdating } = useSettings();

    const enabled = getSetting("local_api_enabled") ?? false;

    return (
      <ToggleSwitch
        checked={enabled}
        onChange={(value) => updateSetting("local_api_enabled", value)}
        isUpdating={isUpdating("local_api_enabled")}
        label={t("settings.advanced.localApiEnabled.label")}
        description={t("settings.advanced.localApiEnabled.description")}
        descriptionMode={descriptionMode}
        grouped={grouped}
      />
    );
  },
);

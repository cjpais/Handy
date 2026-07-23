import React from "react";
import { useTranslation } from "react-i18next";
import { useSettings } from "../../hooks/useSettings";
import { ToggleSwitch } from "../ui/ToggleSwitch";

interface RemoveTrailingPeriodProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const RemoveTrailingPeriod: React.FC<RemoveTrailingPeriodProps> =
  React.memo(({ descriptionMode = "tooltip", grouped = false }) => {
    const { t } = useTranslation();
    const { getSetting, updateSetting, isUpdating } = useSettings();
    const enabled = getSetting("remove_trailing_period") ?? false;

    return (
      <ToggleSwitch
        checked={enabled}
        onChange={(enabled) => updateSetting("remove_trailing_period", enabled)}
        isUpdating={isUpdating("remove_trailing_period")}
        label={t("settings.debug.removeTrailingPeriod.label")}
        description={t("settings.debug.removeTrailingPeriod.description")}
        descriptionMode={descriptionMode}
        grouped={grouped}
      />
    );
  });

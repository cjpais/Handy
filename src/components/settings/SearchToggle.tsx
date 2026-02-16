import React from "react";
import { useTranslation } from "react-i18next";
import { ToggleSwitch } from "../ui/ToggleSwitch";
import { useSettings } from "../../hooks/useSettings";

interface SearchToggleProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const SearchToggle: React.FC<SearchToggleProps> = React.memo(
  ({ descriptionMode = "tooltip", grouped = false }) => {
    const { t } = useTranslation();
    const { getSetting, updateSetting, isUpdating } = useSettings();

    const enabled = getSetting("search_enabled") || false;

    return (
      <ToggleSwitch
        checked={enabled}
        onChange={(enabled) => updateSetting("search_enabled", enabled)}
        isUpdating={isUpdating("search_enabled")}
        label={t("settings.advanced.searchToggle.label")}
        description={t("settings.advanced.searchToggle.description")}
        descriptionMode={descriptionMode}
        grouped={grouped}
      />
    );
  },
);

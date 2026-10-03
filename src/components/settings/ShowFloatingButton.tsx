import React from "react";
import { useTranslation } from "react-i18next";
import { ToggleSwitch } from "../ui/ToggleSwitch";
import { useSettings } from "../../hooks/useSettings";

interface ShowFloatingButtonProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const ShowFloatingButton: React.FC<ShowFloatingButtonProps> = React.memo(
  ({ descriptionMode = "tooltip", grouped = false }) => {
    const { t } = useTranslation();
    const { getSetting, updateSetting, isUpdating } = useSettings();

    const showFloatingButton = getSetting("show_floating_button") ?? false;

    return (
      <ToggleSwitch
        checked={showFloatingButton}
        onChange={(enabled) => updateSetting("show_floating_button", enabled)}
        isUpdating={isUpdating("show_floating_button")}
        label={t("settings.advanced.showFloatingButton.label")}
        description={t("settings.advanced.showFloatingButton.description")}
        descriptionMode={descriptionMode}
        grouped={grouped}
        tooltipPosition="bottom"
      />
    );
  },
);

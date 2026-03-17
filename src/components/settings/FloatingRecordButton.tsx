import React from "react";
import { useTranslation } from "react-i18next";
import { ToggleSwitch } from "../ui/ToggleSwitch";
import { useSettings } from "../../hooks/useSettings";

interface FloatingRecordButtonProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const FloatingRecordButton: React.FC<FloatingRecordButtonProps> =
  React.memo(({ descriptionMode = "tooltip", grouped = false }) => {
    const { t } = useTranslation();
    const { getSetting, updateSetting, isUpdating } = useSettings();

    const showButton = getSetting("show_floating_record_button") ?? false;

    return (
      <ToggleSwitch
        checked={showButton}
        onChange={(enabled) =>
          updateSetting("show_floating_record_button", enabled)
        }
        isUpdating={isUpdating("show_floating_record_button")}
        label={t("settings.advanced.floatingButton.title")}
        description={t("settings.advanced.floatingButton.description")}
        descriptionMode={descriptionMode}
        grouped={grouped}
        tooltipPosition="bottom"
      />
    );
  });

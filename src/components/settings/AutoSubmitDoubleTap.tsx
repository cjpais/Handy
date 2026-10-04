import React from "react";
import { useTranslation } from "react-i18next";
import { ToggleSwitch } from "../ui/ToggleSwitch";
import { useSettings } from "../../hooks/useSettings";

interface AutoSubmitDoubleTapProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const AutoSubmitDoubleTap: React.FC<AutoSubmitDoubleTapProps> =
  React.memo(({ descriptionMode = "tooltip", grouped = false }) => {
    const { t } = useTranslation();
    const { getSetting, updateSetting, isUpdating } = useSettings();

    return (
      <ToggleSwitch
        checked={getSetting("auto_submit_double_tap") ?? false}
        onChange={(enabled) => updateSetting("auto_submit_double_tap", enabled)}
        disabled={
          !getSetting("auto_submit") || getSetting("paste_method") === "none"
        }
        isUpdating={isUpdating("auto_submit_double_tap")}
        label={t("settings.advanced.autoSubmitDoubleTap.title")}
        description={t("settings.advanced.autoSubmitDoubleTap.description")}
        descriptionMode={descriptionMode}
        grouped={grouped}
      />
    );
  });

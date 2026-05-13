import React from "react";
import { useTranslation } from "react-i18next";
import { ToggleSwitch } from "../ui/ToggleSwitch";
import { useSettings } from "../../hooks/useSettings";

interface HighlightTargetWindowProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const HighlightTargetWindow: React.FC<HighlightTargetWindowProps> =
  React.memo(({ descriptionMode = "tooltip", grouped = false }) => {
    const { t } = useTranslation();
    const { getSetting, updateSetting, isUpdating } = useSettings();

    const enabled = getSetting("highlight_target_window") ?? true;

    return (
      <ToggleSwitch
        checked={enabled}
        onChange={(value) => updateSetting("highlight_target_window", value)}
        isUpdating={isUpdating("highlight_target_window")}
        label={t("settings.advanced.highlightTargetWindow.label")}
        description={t("settings.advanced.highlightTargetWindow.description")}
        descriptionMode={descriptionMode}
        grouped={grouped}
      />
    );
  });

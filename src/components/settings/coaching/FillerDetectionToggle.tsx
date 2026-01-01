import React from "react";
import { useTranslation } from "react-i18next";
import { ToggleSwitch } from "../../ui/ToggleSwitch";
import { useSettings } from "../../../hooks/useSettings";

interface FillerDetectionToggleProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const FillerDetectionToggle: React.FC<FillerDetectionToggleProps> = React.memo(
  ({ descriptionMode = "tooltip", grouped = false }) => {
    const { t } = useTranslation();
    const { getSetting, updateSetting, isUpdating } = useSettings();

    const enabled = getSetting("filler_detection_enabled") ?? false;

    return (
      <ToggleSwitch
        checked={enabled}
        onChange={(value) => updateSetting("filler_detection_enabled", value)}
        isUpdating={isUpdating("filler_detection_enabled")}
        label={t("coaching.fillerDetection.label")}
        description={t("coaching.fillerDetection.description")}
        descriptionMode={descriptionMode}
        grouped={grouped}
      />
    );
  },
);

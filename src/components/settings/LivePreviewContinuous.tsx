import React from "react";
import { useTranslation } from "react-i18next";
import { ToggleSwitch } from "../ui/ToggleSwitch";
import { useSettings } from "../../hooks/useSettings";

interface LivePreviewContinuousProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const LivePreviewContinuous: React.FC<LivePreviewContinuousProps> =
  React.memo(({ descriptionMode = "tooltip", grouped = false }) => {
    const { t } = useTranslation();
    const { getSetting, updateSetting, isUpdating } = useSettings();

    const enabled = getSetting("live_preview_continuous") ?? false;

    return (
      <ToggleSwitch
        checked={enabled}
        onChange={(enabled) =>
          updateSetting("live_preview_continuous", enabled)
        }
        isUpdating={isUpdating("live_preview_continuous")}
        label={t("settings.advanced.livePreviewContinuous.label")}
        description={t("settings.advanced.livePreviewContinuous.description")}
        descriptionMode={descriptionMode}
        grouped={grouped}
      />
    );
  });

import React from "react";
import { useTranslation } from "react-i18next";
import { ToggleSwitch } from "../ui/ToggleSwitch";
import { useSettings } from "../../hooks/useSettings";

interface LivePreviewProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const LivePreview: React.FC<LivePreviewProps> = React.memo(
  ({ descriptionMode = "tooltip", grouped = false }) => {
    const { t } = useTranslation();
    const { getSetting, updateSetting, isUpdating } = useSettings();

    const enabled = getSetting("live_preview") ?? false;

    return (
      <ToggleSwitch
        checked={enabled}
        onChange={(enabled) => updateSetting("live_preview", enabled)}
        isUpdating={isUpdating("live_preview")}
        label={t("settings.advanced.livePreview.label")}
        description={t("settings.advanced.livePreview.description")}
        descriptionMode={descriptionMode}
        grouped={grouped}
      />
    );
  },
);

import React from "react";
import { useTranslation } from "react-i18next";
import { ToggleSwitch } from "../ui/ToggleSwitch";
import { useSettings } from "../../hooks/useSettings";

interface HistoryPostProcessToggleProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const HistoryPostProcessToggle: React.FC<HistoryPostProcessToggleProps> =
  React.memo(({ descriptionMode = "tooltip", grouped = false }) => {
    const { t } = useTranslation();
    const { getSetting, updateSetting, isUpdating } = useSettings();

    const enabled = getSetting("history_post_process_enabled") || false;

    return (
      <ToggleSwitch
        checked={enabled}
        onChange={(value) =>
          updateSetting("history_post_process_enabled", value)
        }
        isUpdating={isUpdating("history_post_process_enabled")}
        label={t("settings.debug.historyPostProcessToggle.label")}
        description={t("settings.debug.historyPostProcessToggle.description")}
        descriptionMode={descriptionMode}
        grouped={grouped}
      />
    );
  });

HistoryPostProcessToggle.displayName = "HistoryPostProcessToggle";

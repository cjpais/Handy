import React from "react";
import { useTranslation } from "react-i18next";
import { ToggleSwitch } from "../ui/ToggleSwitch";
import { useSettings } from "../../hooks/useSettings";

interface KeepCancelledRecordingsProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const KeepCancelledRecordings: React.FC<KeepCancelledRecordingsProps> =
  React.memo(({ descriptionMode = "tooltip", grouped = false }) => {
    const { t } = useTranslation();
    const { getSetting, updateSetting, isUpdating } = useSettings();
    const enabled = getSetting("keep_cancelled_recordings") ?? true;

    return (
      <ToggleSwitch
        checked={enabled}
        onChange={(nextEnabled) =>
          updateSetting("keep_cancelled_recordings", nextEnabled)
        }
        isUpdating={isUpdating("keep_cancelled_recordings")}
        label={t("settings.debug.keepCancelledRecordings.title")}
        description={t("settings.debug.keepCancelledRecordings.description")}
        descriptionMode={descriptionMode}
        grouped={grouped}
      />
    );
  });

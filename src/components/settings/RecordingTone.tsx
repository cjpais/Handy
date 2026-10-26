import React from "react";
import { useTranslation } from "react-i18next";
import { ToggleSwitch } from "../ui/ToggleSwitch";
import { useSettings } from "../../hooks/useSettings";

interface RecordingToneProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const RecordingTone: React.FC<RecordingToneProps> = React.memo(
  ({ descriptionMode = "tooltip", grouped = false }) => {
    const { t } = useTranslation();
    const { getSetting, updateSetting, isUpdating } = useSettings();
    const recordingToneEnabled = getSetting("recording_tone") ?? false;

    return (
      <ToggleSwitch
        checked={recordingToneEnabled}
        onChange={(enabled) => updateSetting("recording_tone", enabled)}
        isUpdating={isUpdating("recording_tone")}
        label={t("settings.sound.recordingTone.label")}
        description={t("settings.sound.recordingTone.description")}
        descriptionMode={descriptionMode}
        grouped={grouped}
      />
    );
  },
);

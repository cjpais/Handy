import React from "react";
import { useTranslation } from "react-i18next";
import { ToggleSwitch } from "../ui/ToggleSwitch";
import { useSettings } from "../../hooks/useSettings";

interface PauseMediaWhileRecordingProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const PauseMediaWhileRecording: React.FC<PauseMediaWhileRecordingProps> =
  React.memo(({ descriptionMode = "tooltip", grouped = false }) => {
    const { t } = useTranslation();
    const { getSetting, updateSetting, isUpdating } = useSettings();

    const pauseEnabled = getSetting("pause_media_while_recording") ?? false;

    return (
      <ToggleSwitch
        checked={pauseEnabled}
        onChange={(enabled) =>
          updateSetting("pause_media_while_recording", enabled)
        }
        isUpdating={isUpdating("pause_media_while_recording")}
        label={t("settings.sound.pauseMediaWhileRecording.label")}
        description={t("settings.sound.pauseMediaWhileRecording.description")}
        descriptionMode={descriptionMode}
        grouped={grouped}
      />
    );
  });

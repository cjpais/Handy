import React from "react";
import { useTranslation } from "react-i18next";
import { ToggleSwitch } from "../ui/ToggleSwitch";
import { useSettings } from "../../hooks/useSettings";
import { useOsType } from "../../hooks/useOsType";

interface PauseWhileRecordingToggleProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const PauseWhileRecording: React.FC<PauseWhileRecordingToggleProps> =
  React.memo(({ descriptionMode = "tooltip", grouped = false }) => {
    const { t } = useTranslation();
    const osType = useOsType();
    const { getSetting, updateSetting, isUpdating } = useSettings();

    if (osType !== "macos" && osType !== "windows" && osType !== "linux") {
      return null;
    }

    const pauseEnabled = getSetting("pause_while_recording") ?? false;
    const playAfterRecording = getSetting("play_after_recording") ?? true;

    return (
      <div>
        <ToggleSwitch
          checked={pauseEnabled}
          onChange={async (enabled) => {
            await updateSetting("pause_while_recording", enabled);
            if (!enabled && playAfterRecording) {
              await updateSetting("play_after_recording", false);
            }
          }}
          isUpdating={isUpdating("pause_while_recording")}
          label={t("settings.sound.pauseWhileRecording.label")}
          description={t("settings.sound.pauseWhileRecording.description")}
          descriptionMode={descriptionMode}
          grouped={grouped}
        />
        <div className="-mt-2">
          <ToggleSwitch
            checked={playAfterRecording}
            onChange={(enabled) =>
              updateSetting("play_after_recording", enabled)
            }
            isUpdating={isUpdating("play_after_recording")}
            disabled={!pauseEnabled}
            label={t("settings.sound.playAfterRecording.label")}
            description={t("settings.sound.playAfterRecording.description")}
            descriptionMode={descriptionMode}
            grouped={grouped}
          />
        </div>
      </div>
    );
  });

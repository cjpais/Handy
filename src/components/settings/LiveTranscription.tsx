import React from "react";
import { useTranslation } from "react-i18next";
import { ToggleSwitch } from "../ui/ToggleSwitch";
import { useSettings } from "../../hooks/useSettings";

interface LiveTranscriptionProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const LiveTranscription: React.FC<LiveTranscriptionProps> = React.memo(
  ({ descriptionMode = "tooltip", grouped = false }) => {
    const { t } = useTranslation();
    const { getSetting, updateSetting, isUpdating } = useSettings();

    const liveEnabled = getSetting("live_transcription_enabled") || false;
    const postProcessEnabled = getSetting("post_process_enabled") || false;

    return (
      <ToggleSwitch
        checked={liveEnabled}
        onChange={(enabled) =>
          updateSetting("live_transcription_enabled", enabled)
        }
        isUpdating={isUpdating("live_transcription_enabled")}
        label={t("settings.general.liveTranscription.label")}
        description={
          postProcessEnabled && !liveEnabled
            ? t("settings.general.liveTranscription.descriptionDisabled")
            : t("settings.general.liveTranscription.description")
        }
        descriptionMode={descriptionMode}
        grouped={grouped}
      />
    );
  },
);

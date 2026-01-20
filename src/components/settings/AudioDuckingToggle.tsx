import React from "react";
import { useTranslation } from "react-i18next";
import { ToggleSwitch } from "../ui/ToggleSwitch";
import { useSettings } from "../../hooks/useSettings";

interface AudioDuckingToggleProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const AudioDuckingToggle: React.FC<AudioDuckingToggleProps> = React.memo(
  ({ descriptionMode = "tooltip", grouped = false }) => {
    const { t } = useTranslation();
    const { getSetting, updateSetting, isUpdating } = useSettings();

    const duckingEnabled = getSetting("audio_ducking_enabled") ?? false;

    return (
      <ToggleSwitch
        checked={duckingEnabled}
        onChange={(enabled) => updateSetting("audio_ducking_enabled", enabled)}
        isUpdating={isUpdating("audio_ducking_enabled")}
        label={t("settings.sound.audioDucking.label")}
        description={t("settings.sound.audioDucking.description")}
        descriptionMode={descriptionMode}
        grouped={grouped}
      />
    );
  },
);

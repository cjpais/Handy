import React from "react";
import { useTranslation } from "react-i18next";
import { Slider } from "../ui/Slider";
import { useSettings } from "../../hooks/useSettings";

export const RecordingVolumeSlider: React.FC<{ disabled?: boolean }> = ({
  disabled = false,
}) => {
  const { t } = useTranslation();
  const { getSetting, updateSetting } = useSettings();
  const duckVolume = getSetting("recording_duck_volume") ?? 0;

  return (
    <Slider
      value={duckVolume}
      onChange={(value: number) =>
        updateSetting("recording_duck_volume", Math.round(value))
      }
      min={0}
      max={90}
      step={5}
      label={t("settings.sound.recordingVolume.title")}
      description={t("settings.sound.recordingVolume.description")}
      descriptionMode="tooltip"
      grouped
      formatValue={(value) =>
        value === 0
          ? t("settings.sound.recordingVolume.muted")
          : `${Math.round(value)}%`
      }
      disabled={disabled}
    />
  );
};

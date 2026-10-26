import React from "react";
import { useTranslation } from "react-i18next";
import { Slider } from "../ui/Slider";
import { useSettings } from "../../hooks/useSettings";

export const RecordingToneVolume: React.FC<{ disabled?: boolean }> = ({
  disabled = false,
}) => {
  const { t } = useTranslation();
  const { getSetting, updateSetting } = useSettings();
  const recordingToneVolume = getSetting("recording_tone_volume") ?? 0.2;

  return (
    <Slider
      value={recordingToneVolume}
      onChange={(value: number) =>
        updateSetting("recording_tone_volume", value)
      }
      min={0}
      max={1}
      label={t("settings.sound.recordingToneVolume.title")}
      description={t("settings.sound.recordingToneVolume.description")}
      descriptionMode="tooltip"
      grouped
      formatValue={(value) => `${Math.round(value * 100)}%`}
      disabled={disabled}
    />
  );
};

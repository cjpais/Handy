import React from "react";
import { Slider } from "../ui/Slider";
import { useSettings } from "../../hooks/useSettings";
import { useTranslation } from "react-i18next";

export const VolumeSlider: React.FC<{ disabled?: boolean }> = ({
  disabled = false,
}) => {
  const { t } = useTranslation();
  const { getSetting, updateSetting } = useSettings();
  const audioFeedbackVolume = getSetting("audio_feedback_volume") ?? 0.5;

  return (
    <Slider
      value={audioFeedbackVolume}
      onChange={(value: number) =>
        updateSetting("audio_feedback_volume", value)
      }
      min={0}
      max={1}
      step={0.1}
      label={t("settings.general.volume.label")}
      description={t("settings.general.volume.description")}
      descriptionMode="tooltip"
      grouped
      formatValue={(value) =>
        t("settings.general.volume.format", {
          value: Math.round(value * 100),
        })
      }
      disabled={disabled}
    />
  );
};

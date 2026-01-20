import React from "react";
import { useTranslation } from "react-i18next";
import { Slider } from "../ui/Slider";
import { useSettings } from "../../hooks/useSettings";

export const DuckingLevelSlider: React.FC<{ disabled?: boolean }> = ({
  disabled = false,
}) => {
  const { t } = useTranslation();
  const { getSetting, updateSetting } = useSettings();
  const duckingLevel = getSetting("audio_ducking_level") ?? 0.2;

  return (
    <Slider
      value={duckingLevel}
      onChange={(value: number) => updateSetting("audio_ducking_level", value)}
      min={0}
      max={1}
      step={0.1}
      label={t("settings.sound.duckingLevel.title")}
      description={t("settings.sound.duckingLevel.description")}
      descriptionMode="tooltip"
      grouped
      formatValue={(value) => `${Math.round(value * 100)}%`}
      disabled={disabled}
    />
  );
};

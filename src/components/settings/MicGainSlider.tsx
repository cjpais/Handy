import React from "react";
import { useTranslation } from "react-i18next";
import { Slider } from "../ui/Slider";
import { useSettings } from "../../hooks/useSettings";

export const MicGainSlider: React.FC<{ disabled?: boolean }> = ({
  disabled = false,
}) => {
  const { t } = useTranslation();
  const { getSetting, updateSetting } = useSettings();
  const micGain = getSetting("mic_gain") ?? 1.0;

  return (
    <Slider
      value={micGain}
      onChange={(value: number) => updateSetting("mic_gain", value)}
      min={1}
      max={4}
      step={0.1}
      label={t("settings.sound.micGain.title")}
      description={t("settings.sound.micGain.description")}
      descriptionMode="tooltip"
      grouped
      formatValue={(value) => `${value.toFixed(1)}×`}
      disabled={disabled}
    />
  );
};

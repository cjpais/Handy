import React from "react";
import { useTranslation } from "react-i18next";
import { Slider } from "../../ui/Slider";
import { useSettings } from "../../../hooks/useSettings";

interface DoubleTapDelayProps {
  descriptionMode?: "tooltip" | "inline";
  grouped?: boolean;
}

export const DoubleTapDelay: React.FC<DoubleTapDelayProps> = ({
  descriptionMode = "tooltip",
  grouped = false,
}) => {
  const { t } = useTranslation();
  const { settings, updateSetting } = useSettings();

  const handleDelayChange = (value: number) => {
    updateSetting("double_tap_delay_ms", value);
  };

  return (
    <Slider
      value={settings?.double_tap_delay_ms ?? 400}
      onChange={handleDelayChange}
      min={100}
      max={1000}
      step={50}
      label={t("settings.debug.doubleTapDelay.title")}
      description={t("settings.debug.doubleTapDelay.description")}
      descriptionMode={descriptionMode}
      grouped={grouped}
      formatValue={(v) => `${v}ms`}
    />
  );
};

import React from "react";
import { useTranslation } from "react-i18next";
import { Slider } from "../../ui/Slider";
import { useSettings } from "../../../hooks/useSettings";

interface TypingDelayProps {
  descriptionMode?: "tooltip" | "inline";
  grouped?: boolean;
}

export const TypingDelay: React.FC<TypingDelayProps> = ({
  descriptionMode = "tooltip",
  grouped = false,
}) => {
  const { t } = useTranslation();
  const { settings, updateSetting } = useSettings();

  const handleDelayChange = (value: number) => {
    updateSetting("typing_delay_ms", value);
  };

  return (
    <Slider
      value={settings?.typing_delay_ms ?? 2}
      onChange={handleDelayChange}
      min={0}
      max={50}
      step={1}
      label={t("settings.debug.typingDelay.title")}
      description={t("settings.debug.typingDelay.description")}
      descriptionMode={descriptionMode}
      grouped={grouped}
      formatValue={(v) => `${v}ms`}
    />
  );
};

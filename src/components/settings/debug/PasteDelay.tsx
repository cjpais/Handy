import React from "react";
import { useTranslation } from "react-i18next";
import { Slider } from "../../ui/Slider";
import { useSettings } from "../../../hooks/useSettings";

type PasteDelayKey = "paste_delay_ms" | "paste_delay_after_ms";

interface PasteDelayProps {
  descriptionMode?: "tooltip" | "inline";
  grouped?: boolean;
  settingKey?: PasteDelayKey;
  labelKey?: string;
  descriptionKey?: string;
}

export const PasteDelay: React.FC<PasteDelayProps> = ({
  descriptionMode = "tooltip",
  grouped = false,
  settingKey = "paste_delay_ms",
  labelKey = "settings.debug.pasteDelay.title",
  descriptionKey = "settings.debug.pasteDelay.description",
}) => {
  const { t } = useTranslation();
  const { settings, updateSetting } = useSettings();

  const handleDelayChange = (value: number) => {
    updateSetting(settingKey, value);
  };

  return (
    <Slider
      value={settings?.[settingKey] ?? 60}
      onChange={handleDelayChange}
      min={10}
      max={500}
      step={10}
      label={t(labelKey)}
      description={t(descriptionKey)}
      descriptionMode={descriptionMode}
      grouped={grouped}
      formatValue={(v) => `${v}ms`}
    />
  );
};

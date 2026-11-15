import React from "react";
import { useTranslation } from "react-i18next";
import { Slider } from "../../ui/Slider";
import { useSettings } from "../../../hooks/useSettings";
import { useOsType } from "../../../hooks/useOsType";

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
  const { settings, updateSetting, resetSetting, isUpdating } = useSettings();
  const osType = useOsType();

  // macOS restores the clipboard in the background, after the clipboard
  // restore delay; the delay after the paste keystroke is not used there.
  if (osType === "macos" && settingKey === "paste_delay_after_ms") {
    return null;
  }

  const handleDelayChange = (value: number) => {
    updateSetting(settingKey, value);
  };

  return (
    <Slider
      value={settings?.[settingKey] ?? 60}
      onChange={handleDelayChange}
      onReset={() => resetSetting(settingKey)}
      isResetting={isUpdating(settingKey)}
      min={10}
      max={5000}
      step={10}
      label={t(labelKey)}
      description={t(descriptionKey)}
      descriptionMode={descriptionMode}
      grouped={grouped}
      formatValue={(v) => `${v}ms`}
    />
  );
};

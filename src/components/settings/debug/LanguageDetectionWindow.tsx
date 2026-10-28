import React from "react";
import { useTranslation } from "react-i18next";
import { Slider } from "../../ui/Slider";
import { useSettings } from "../../../hooks/useSettings";

interface LanguageDetectionWindowProps {
  descriptionMode?: "tooltip" | "inline";
  grouped?: boolean;
}

export const LanguageDetectionWindow: React.FC<
  LanguageDetectionWindowProps
> = ({ descriptionMode = "tooltip", grouped = false }) => {
  const { t } = useTranslation();
  const { settings, updateSetting, resetSetting, isUpdating } = useSettings();

  return (
    <Slider
      value={settings?.language_detection_window_secs ?? 5}
      onChange={(value) =>
        updateSetting("language_detection_window_secs", value)
      }
      onReset={() => resetSetting("language_detection_window_secs")}
      isResetting={isUpdating("language_detection_window_secs")}
      min={2}
      max={30}
      step={1}
      label={t("settings.debug.languageDetectionWindow.title")}
      description={t("settings.debug.languageDetectionWindow.description")}
      descriptionMode={descriptionMode}
      grouped={grouped}
      formatValue={(v) => `${v}s`}
    />
  );
};

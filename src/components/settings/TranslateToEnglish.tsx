import React from "react";
import { ToggleSwitch } from "../ui/ToggleSwitch";
import { useSettings } from "../../hooks/useSettings";

interface TranslateToEnglishProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const TranslateToEnglish: React.FC<TranslateToEnglishProps> = React.memo(({
  descriptionMode = "tooltip",
  grouped = false,
}) => {
  const { getSetting, updateSetting, isUpdating } = useSettings();

  const translateToEnglish = getSetting("translate_to_english") || false;

  return (
    <ToggleSwitch
      checked={translateToEnglish}
      onChange={(enabled) => updateSetting("translate_to_english", enabled)}
      isUpdating={isUpdating("translate_to_english")}
      label="Translate to English"
      description="Automatically translate speech from other languages to English during transcription."
      descriptionMode={descriptionMode}
      grouped={grouped}
    />
  );
});

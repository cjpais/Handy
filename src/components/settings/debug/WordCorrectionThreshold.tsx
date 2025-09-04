import React from "react";
import { Slider } from "../../ui/Slider";
import { useSettings } from "../../../hooks/useSettings";

interface WordCorrectionThresholdProps {
  descriptionMode?: "tooltip" | "inline";
  grouped?: boolean;
}

export const WordCorrectionThreshold: React.FC<
  WordCorrectionThresholdProps
> = ({ descriptionMode = "tooltip", grouped = false }) => {
  const { settings, updateSetting } = useSettings();

  const handleThresholdChange = (value: number) => {
    updateSetting("word_correction_threshold", value);
  };

  return (
    <Slider
      value={settings?.word_correction_threshold ?? 0.18}
      onChange={handleThresholdChange}
      min={0.0}
      max={1.0}
      label="Word Correction Threshold"
      description="Controls how aggressively custom words are applied. Lower values mean fewer corrections will be made, higher values mean more corrections. Range: 0 (least aggressive) to 1 (most aggressive)."
      descriptionMode={descriptionMode}
      grouped={grouped}
    />
  );
};

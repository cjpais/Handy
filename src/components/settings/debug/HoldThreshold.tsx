import React from "react";
import { useTranslation } from "react-i18next";
import { Slider } from "../../ui/Slider";
import { useSettings } from "../../../hooks/useSettings";

interface HoldThresholdProps {
  descriptionMode?: "tooltip" | "inline";
  grouped?: boolean;
}

/**
 * Hold-or-toggle only: how long the transcribe key must be held before the
 * press counts as push-to-talk instead of a tap.
 */
export const HoldThreshold: React.FC<HoldThresholdProps> = ({
  descriptionMode = "tooltip",
  grouped = false,
}) => {
  const { t } = useTranslation();
  const { settings, updateSetting, resetSetting, isUpdating } = useSettings();

  return (
    <Slider
      value={settings?.hold_threshold_ms ?? 300}
      onChange={(value) => updateSetting("hold_threshold_ms", value)}
      onReset={() => resetSetting("hold_threshold_ms")}
      isResetting={isUpdating("hold_threshold_ms")}
      min={100}
      max={1000}
      step={50}
      label={t("settings.debug.holdThreshold.title")}
      description={t("settings.debug.holdThreshold.description")}
      descriptionMode={descriptionMode}
      grouped={grouped}
      formatValue={(v) => `${v}ms`}
    />
  );
};

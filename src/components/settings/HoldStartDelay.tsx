import React from "react";
import { useTranslation } from "react-i18next";
import { Slider } from "../ui/Slider";
import { useSettings } from "../../hooks/useSettings";

interface HoldStartDelayProps {
  descriptionMode?: "tooltip" | "inline";
  grouped?: boolean;
}

/**
 * Hold (push-to-talk) only: how long the transcribe key must be held before
 * recording starts. Any other key pressed while it's held cancels.
 */
export const HoldStartDelay: React.FC<HoldStartDelayProps> = ({
  descriptionMode = "tooltip",
  grouped = false,
}) => {
  const { t } = useTranslation();
  const { settings, updateSetting, resetSetting, isUpdating } = useSettings();

  return (
    <Slider
      value={settings?.push_to_talk_start_delay_ms ?? 0}
      onChange={(value) => updateSetting("push_to_talk_start_delay_ms", value)}
      onReset={() => resetSetting("push_to_talk_start_delay_ms")}
      isResetting={isUpdating("push_to_talk_start_delay_ms")}
      min={0}
      max={1000}
      step={50}
      label={t("settings.general.holdStartDelay.title")}
      description={t("settings.general.holdStartDelay.description")}
      descriptionMode={descriptionMode}
      grouped={grouped}
      formatValue={(v) => `${v}ms`}
    />
  );
};

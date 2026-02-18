import React from "react";
import { useTranslation } from "react-i18next";
import { Slider } from "../ui/Slider";
import { useSettings } from "../../hooks/useSettings";

export const AudioSpeed: React.FC<{ disabled?: boolean; grouped?: boolean }> = ({
  disabled = false,
  grouped = false,
}) => {
  const { t } = useTranslation();
  const { getSetting, updateSetting } = useSettings();
  const audioSpeed = getSetting("audio_speed") ?? 1.0;

  // Warning shown only when speed > 1.0
  const showWarning = audioSpeed > 1.0;

  return (
    <div className="space-y-2">
      <Slider
        value={audioSpeed}
        onChange={(value: number) => updateSetting("audio_speed", value)}
        min={1.0}
        max={2.0}
        step={0.1}
        label={t("settings.sound.audioSpeed.title")}
        description={t("settings.sound.audioSpeed.description")}
        descriptionMode="tooltip"
        grouped={grouped}
        formatValue={(value) => `${value.toFixed(1)}x`}
        disabled={disabled}
      />
      {showWarning && (
        <p className="text-xs text-yellow-500 px-3">
          {t("settings.sound.audioSpeed.warning")}
        </p>
      )}
    </div>
  );
};

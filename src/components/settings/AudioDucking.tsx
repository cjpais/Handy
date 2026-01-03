import React from "react";
import { useTranslation } from "react-i18next";
import { ToggleSwitch } from "../ui/ToggleSwitch";
import { Slider } from "../ui/Slider";
import { useSettings } from "../../hooks/useSettings";

interface AudioDuckingProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const AudioDucking: React.FC<AudioDuckingProps> = React.memo(
  ({ descriptionMode = "tooltip", grouped = false }) => {
    const { t } = useTranslation();
    const { getSetting, updateSetting, isUpdating } = useSettings();

    const duckingEnabled = getSetting("audio_ducking_enabled") ?? false;
    const duckingAmount = getSetting("audio_ducking_amount") ?? 1.0;

    return (
      <>
        <ToggleSwitch
          checked={duckingEnabled}
          onChange={(enabled) =>
            updateSetting("audio_ducking_enabled", enabled)
          }
          isUpdating={isUpdating("audio_ducking_enabled")}
          label={t("settings.sound.audioDucking.label")}
          description={t("settings.sound.audioDucking.description")}
          descriptionMode={descriptionMode}
          grouped={grouped}
        />
        {duckingEnabled && (
          <Slider
            value={duckingAmount}
            onChange={(value: number) =>
              updateSetting("audio_ducking_amount", value)
            }
            min={0}
            max={1}
            step={0.1}
            label={t("settings.sound.audioDucking.reductionLabel")}
            description={t("settings.sound.audioDucking.reductionDescription")}
            descriptionMode={descriptionMode}
            grouped={grouped}
            formatValue={(v) =>
              v === 1
                ? t("settings.sound.audioDucking.muted")
                : v === 0
                  ? t("settings.sound.audioDucking.noChange")
                  : `${Math.round(v * 100)}%`
            }
          />
        )}
      </>
    );
  }
);

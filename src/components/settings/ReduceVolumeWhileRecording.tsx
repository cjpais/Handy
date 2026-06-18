import React from "react";
import { useTranslation } from "react-i18next";
import { ToggleSwitch } from "../ui/ToggleSwitch";
import { Slider } from "../ui/Slider";
import { useSettings } from "../../hooks/useSettings";

interface ReduceVolumeWhileRecordingProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const ReduceVolumeWhileRecording: React.FC<ReduceVolumeWhileRecordingProps> =
  React.memo(({ descriptionMode = "tooltip", grouped = false }) => {
    const { t } = useTranslation();
    const { getSetting, updateSetting, isUpdating } = useSettings();

    const reductionEnabled =
      getSetting("reduce_volume_while_recording") ?? false;
    const reductionPercent =
      getSetting("recording_volume_reduction_percent") ?? 75;
    const fadeMs = getSetting("recording_volume_fade_ms") ?? 300;

    return (
      <>
        <ToggleSwitch
          checked={reductionEnabled}
          onChange={(enabled) =>
            updateSetting("reduce_volume_while_recording", enabled)
          }
          isUpdating={isUpdating("reduce_volume_while_recording")}
          label={t("settings.debug.reduceVolumeWhileRecording.label")}
          description={t(
            "settings.debug.reduceVolumeWhileRecording.description",
          )}
          descriptionMode={descriptionMode}
          grouped={grouped}
        />
        {reductionEnabled && (
          <Slider
            value={reductionPercent}
            onChange={(value) =>
              updateSetting("recording_volume_reduction_percent", value)
            }
            min={0}
            max={100}
            step={5}
            label={t("settings.debug.recordingVolumeReduction.label")}
            description={t(
              "settings.debug.recordingVolumeReduction.description",
            )}
            descriptionMode={descriptionMode}
            grouped={grouped}
            formatValue={(value) => `${Math.round(value)}%`}
          />
        )}
        {reductionEnabled && (
          <Slider
            value={fadeMs}
            onChange={(value) =>
              updateSetting("recording_volume_fade_ms", value)
            }
            min={0}
            max={2000}
            step={100}
            label={t("settings.debug.recordingVolumeFade.label")}
            description={t("settings.debug.recordingVolumeFade.description")}
            descriptionMode={descriptionMode}
            grouped={grouped}
            formatValue={(value) => `${Math.round(value)} ms`}
          />
        )}
      </>
    );
  });

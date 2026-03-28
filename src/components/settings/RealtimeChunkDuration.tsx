import React from "react";
import { useTranslation } from "react-i18next";
import { Slider } from "../ui/Slider";
import { useSettings } from "../../hooks/useSettings";

interface RealtimeChunkDurationProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const RealtimeChunkDuration: React.FC<RealtimeChunkDurationProps> =
  React.memo(({ descriptionMode = "tooltip", grouped = false }) => {
    const { t } = useTranslation();
    const { getSetting, updateSetting } = useSettings();
    const duration = getSetting("realtime_chunk_duration_secs") ?? 3.0;

    return (
      <Slider
        value={duration}
        onChange={(value: number) =>
          updateSetting("realtime_chunk_duration_secs", value)
        }
        min={1}
        max={10}
        step={0.5}
        label={t("settings.experimental.realtimeChunkDuration.title")}
        description={t(
          "settings.experimental.realtimeChunkDuration.description",
        )}
        descriptionMode={descriptionMode}
        grouped={grouped}
        formatValue={(value) => `${value}s`}
      />
    );
  });

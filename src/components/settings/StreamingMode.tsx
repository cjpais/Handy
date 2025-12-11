import React from "react";
import { ToggleSwitch } from "../ui/ToggleSwitch";
import { useSettings } from "../../hooks/useSettings";

interface StreamingModeProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const StreamingMode: React.FC<StreamingModeProps> = React.memo(
  ({ descriptionMode = "tooltip", grouped = false }) => {
    const { getSetting, updateSetting, isUpdating } = useSettings();

    const enabled = getSetting("streaming_mode_enabled") ?? false;

    return (
      <ToggleSwitch
        checked={enabled}
        onChange={(enabled) => updateSetting("streaming_mode_enabled", enabled)}
        isUpdating={isUpdating("streaming_mode_enabled")}
        label="Streaming Mode"
        description="Output transcription incrementally while speaking. Text appears at natural pause points, updating as you continue speaking. Best for local models with fast inference."
        descriptionMode={descriptionMode}
        grouped={grouped}
      />
    );
  }
);

import React, { useEffect, useMemo } from "react";
import { ToggleSwitch } from "../ui/ToggleSwitch";
import { useSettings } from "../../hooks/useSettings";

interface SaveToHistoryProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const SaveToHistory: React.FC<SaveToHistoryProps> = React.memo(
  ({ descriptionMode = "tooltip", grouped = false }) => {
    const { getSetting, updateSetting, isUpdating } = useSettings();

    const SaveToHistory = getSetting("save_to_history") ?? true;

    const description = "Save recordings and transcriptions to history." ;

    return (
      <ToggleSwitch
        checked={SaveToHistory}
        onChange={(enabled) => updateSetting("save_to_history", enabled)}
        isUpdating={isUpdating("save_to_history")}
        label="Save to history"
        description={description}
        descriptionMode={descriptionMode}
        grouped={grouped}
      />
    );
  }
);

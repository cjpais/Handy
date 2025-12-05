import React from "react";
import { ToggleSwitch } from "../ui/ToggleSwitch";
import { useSettings } from "../../hooks/useSettings";

interface ContextAwareCapitalizationToggleProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const ContextAwareCapitalization: React.FC<ContextAwareCapitalizationToggleProps> =
  React.memo(({ descriptionMode = "tooltip", grouped = false }) => {
    const { getSetting, updateSetting, isUpdating } = useSettings();

    const enabled = getSetting("context_aware_capitalization") ?? true;

    return (
      <ToggleSwitch
        checked={enabled}
        onChange={(enabled) =>
          updateSetting("context_aware_capitalization", enabled)
        }
        isUpdating={isUpdating("context_aware_capitalization")}
        label="Context-Aware Capitalization"
        description="Automatically adjust capitalization based on surrounding text. Capitalizes after periods, lowercases after commas."
        descriptionMode={descriptionMode}
        grouped={grouped}
      />
    );
  });

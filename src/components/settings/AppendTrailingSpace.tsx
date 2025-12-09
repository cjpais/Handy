import React from "react";
import { ToggleSwitch } from "../ui/ToggleSwitch";
import { useSettings } from "../../hooks/useSettings";

interface AppendTrailingSpaceProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const AppendTrailingSpace: React.FC<AppendTrailingSpaceProps> =
  React.memo(({ descriptionMode = "tooltip", grouped = false }) => {
    const { getSetting, updateSetting, isUpdating } = useSettings();

    const enabled = getSetting("append_trailing_space") ?? false;

    return (
      <ToggleSwitch
        checked={enabled}
        onChange={(enabled) => updateSetting("append_trailing_space", enabled)}
        isUpdating={isUpdating("append_trailing_space")}
        label="Append Trailing Space"
        description="Automatically add a space at the end of transcribed text, making it easier to dictate multiple sentences in a row."
        descriptionMode={descriptionMode}
        grouped={grouped}
      />
    );
  });

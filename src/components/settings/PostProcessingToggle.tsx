import React from "react";
import { ToggleSwitch } from "../ui/ToggleSwitch";
import { useSettings } from "../../hooks/useSettings";

interface PostProcessingToggleProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const PostProcessingToggle: React.FC<PostProcessingToggleProps> =
  React.memo(({ descriptionMode = "tooltip", grouped = false }) => {
    const { getSetting, updateSetting, isUpdating } = useSettings();

    const enabled = getSetting("post_process_enabled") || false;

    return (
      <ToggleSwitch
        checked={enabled}
        onChange={(enabled) => updateSetting("post_process_enabled", enabled)}
        isUpdating={isUpdating("post_process_enabled")}
        label="Post Process"
        description="Enable post-processing of transcribed text using language models via OpenAI Compatible API."
        descriptionMode={descriptionMode}
        grouped={grouped}
      />
    );
  });

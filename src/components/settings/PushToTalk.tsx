import React from "react";
import { ToggleSwitch } from "../ui/ToggleSwitch";
import { useSettings } from "../../hooks/useSettings";

interface PushToTalkProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const PushToTalk: React.FC<PushToTalkProps> = React.memo(
  ({ descriptionMode = "tooltip", grouped = false }) => {
    const { getSetting, updateSetting, isUpdating } = useSettings();

    const pttEnabled = getSetting("push_to_talk") || false;

    return (
      <ToggleSwitch
        checked={pttEnabled}
        onChange={(enabled) => updateSetting("push_to_talk", enabled)}
        isUpdating={isUpdating("push_to_talk")}
        label="Push To Talk"
        description="Hold to record, release to stop"
        descriptionMode={descriptionMode}
        grouped={grouped}
      />
    );
  },
);

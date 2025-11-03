import React from "react";
import { ToggleSwitch } from "../ui/ToggleSwitch";
import { useSettings } from "../../hooks/useSettings";

interface MuteWhileRecordingToggleProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const MuteWhileRecording: React.FC<MuteWhileRecordingToggleProps> =
  React.memo(({ descriptionMode = "tooltip", grouped = false }) => {
    const { getSetting, updateSetting, isUpdating } = useSettings();

    const muteEnabled = getSetting("mute_while_recording") ?? false;

    return (
      <ToggleSwitch
        checked={muteEnabled}
        onChange={(enabled) => updateSetting("mute_while_recording", enabled)}
        isUpdating={isUpdating("mute_while_recording")}
        label="Mute While Recording"
        description="Automatically mute all sound output while Handy is recording, then restore it when finished."
        descriptionMode={descriptionMode}
        grouped={grouped}
      />
    );
  });

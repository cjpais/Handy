import React, { useMemo } from "react";
import { useSettings } from "../../hooks/useSettings";
import { Dropdown } from "../ui/Dropdown";
import { SettingContainer } from "../ui/SettingContainer";
import type { MicrophoneKeepAlive } from "../../lib/types";

const baseOptions: { value: MicrophoneKeepAlive; label: string }[] = [
  { value: "off", label: "Only while recording" },
  { value: "sec15", label: "Keep alive for 15 seconds" },
  { value: "sec30", label: "Keep alive for 30 seconds" },
  { value: "min1", label: "Keep alive for 1 minute" },
  { value: "min5", label: "Keep alive for 5 minutes" },
  { value: "min15", label: "Keep alive for 15 minutes" },
  { value: "hour1", label: "Keep alive for 1 hour" },
  { value: "forever", label: "Forever" },
];

const debugOptions: { value: MicrophoneKeepAlive; label: string }[] = [
  { value: "sec5", label: "Keep alive for 5 seconds (Debug)" },
  ...baseOptions,
];

interface AlwaysOnMicrophoneProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const AlwaysOnMicrophone: React.FC<AlwaysOnMicrophoneProps> = React.memo(({
  descriptionMode = "tooltip",
  grouped = false,
}) => {
  const { settings, getSetting, updateSetting, isUpdating } = useSettings();

  const currentValue = (getSetting("microphone_keep_alive") || "off") as MicrophoneKeepAlive;

  const options = useMemo(() => {
    return settings?.debug_mode ? debugOptions : baseOptions;
  }, [settings]);

  return (
    <SettingContainer
      title="Microphone Keep-Alive"
      description="Control how long we keep the microphone warm after a recording. Longer times reduce latency but keep the mic active."
      descriptionMode={descriptionMode}
      grouped={grouped}
    >
      <Dropdown
        options={options}
        selectedValue={currentValue}
        onSelect={(value) =>
          updateSetting("microphone_keep_alive", value as MicrophoneKeepAlive)
        }
        disabled={isUpdating("microphone_keep_alive")}
      />
    </SettingContainer>
  );
});

import React from "react";
import { Dropdown } from "../ui/Dropdown";
import { SettingContainer } from "../ui/SettingContainer";
import { ResetButton } from "../ui/ResetButton";
import { useSettings } from "../../hooks/useSettings";

interface ClamshellMicrophoneSelectorProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const ClamshellMicrophoneSelector: React.FC<ClamshellMicrophoneSelectorProps> = React.memo(
  ({ descriptionMode = "tooltip", grouped = false }) => {
    const {
      getSetting,
      updateSetting,
      resetSetting,
      isUpdating,
      isLoading,
      audioDevices,
      refreshAudioDevices,
    } = useSettings();

    const selectedClamshellMicrophone =
      getSetting("clamshell_microphone") === "default"
        ? "Default"
        : getSetting("clamshell_microphone") || "Default";

    const handleClamshellMicrophoneSelect = async (deviceName: string) => {
      await updateSetting("clamshell_microphone", deviceName);
    };

    const handleReset = async () => {
      await resetSetting("clamshell_microphone");
    };

    const microphoneOptions = audioDevices.map((device) => ({
      value: device.name,
      label: device.name,
    }));

    return (
      <SettingContainer
        title="Desktop Microphone"
        description="Choose a different microphone to use when your laptop lid is closed"
        descriptionMode={descriptionMode}
        grouped={grouped}
      >
        <div className="flex items-center space-x-1">
          <Dropdown
            options={microphoneOptions}
            selectedValue={selectedClamshellMicrophone}
            onSelect={handleClamshellMicrophoneSelect}
            placeholder={
              isLoading || audioDevices.length === 0
                ? "Loading..."
                : "Select microphone..."
            }
            disabled={
              isUpdating("clamshell_microphone") ||
              isLoading ||
              audioDevices.length === 0
            }
            onRefresh={refreshAudioDevices}
          />
          <ResetButton
            onClick={handleReset}
            disabled={isUpdating("clamshell_microphone") || isLoading}
          />
        </div>
      </SettingContainer>
    );
  },
);

ClamshellMicrophoneSelector.displayName = "ClamshellMicrophoneSelector";
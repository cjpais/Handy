import React from "react";
import { useTranslation } from "react-i18next";
import { Dropdown } from "../ui/Dropdown";
import { SettingContainer } from "../ui/SettingContainer";
import { ResetButton } from "../ui/ResetButton";
import { useSettings } from "../../hooks/useSettings";

interface MicrophoneSelectorProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

/** The microphone list, as a dropdown: Settings' selector, and Try it's. */
export const MicrophoneDropdown: React.FC<{
  placement?: "below" | "above";
  tone?: "brand" | "neutral";
  /** After a microphone is picked and saved. */
  onPicked?: () => void;
}> = ({ placement, tone, onPicked }) => {
  const { t } = useTranslation();
  const {
    getSetting,
    updateSetting,
    isUpdating,
    isLoading,
    audioDevices,
    refreshAudioDevices,
  } = useSettings();

  const selectedMicrophone =
    getSetting("selected_microphone") === "default"
      ? "Default"
      : getSetting("selected_microphone") || "Default";

  const handleMicrophoneSelect = async (deviceName: string) => {
    await updateSetting("selected_microphone", deviceName);
    onPicked?.();
  };

  const microphoneOptions = audioDevices.map((device) => ({
    value: device.name,
    label: device.name,
  }));

  return (
    <Dropdown
      options={microphoneOptions}
      selectedValue={selectedMicrophone}
      onSelect={handleMicrophoneSelect}
      placeholder={
        isLoading || audioDevices.length === 0
          ? t("settings.sound.microphone.loading")
          : t("settings.sound.microphone.placeholder")
      }
      disabled={
        isUpdating("selected_microphone") ||
        isLoading ||
        audioDevices.length === 0
      }
      onOpen={refreshAudioDevices}
      placement={placement}
      tone={tone}
    />
  );
};

export const MicrophoneSelector: React.FC<MicrophoneSelectorProps> = React.memo(
  ({ descriptionMode = "tooltip", grouped = false }) => {
    const { t } = useTranslation();
    const { resetSetting, isUpdating, isLoading } = useSettings();

    const handleReset = async () => {
      await resetSetting("selected_microphone");
    };

    return (
      <SettingContainer
        title={t("settings.sound.microphone.title")}
        description={t("settings.sound.microphone.description")}
        descriptionMode={descriptionMode}
        grouped={grouped}
      >
        <div className="flex items-center space-x-1">
          <MicrophoneDropdown />
          <ResetButton
            onClick={handleReset}
            disabled={isUpdating("selected_microphone") || isLoading}
          />
        </div>
      </SettingContainer>
    );
  },
);

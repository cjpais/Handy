import React from "react";
import { useTranslation } from "react-i18next";
import { SettingContainer } from "../../ui/SettingContainer";
import { Dropdown, type DropdownOption } from "../../ui/Dropdown";
import { useSettings } from "../../../hooks/useSettings";
import type { StreamingAudioMode as StreamingAudioModeValue } from "../../../bindings";

interface StreamingAudioModeProps {
  descriptionMode?: "tooltip" | "inline";
  grouped?: boolean;
}

export const StreamingAudioMode: React.FC<StreamingAudioModeProps> = ({
  descriptionMode = "tooltip",
  grouped = false,
}) => {
  const { t } = useTranslation();
  const { settings, updateSetting, isUpdating } = useSettings();
  const currentMode = settings?.streaming_audio_mode ?? "continuous";

  const options: DropdownOption[] = [
    {
      value: "continuous",
      label: t("settings.debug.streamingAudioMode.options.continuous"),
    },
    {
      value: "gated",
      label: t("settings.debug.streamingAudioMode.options.gated"),
    },
  ];

  const handleSelect = async (value: string) => {
    if (value === currentMode) return;
    try {
      await updateSetting(
        "streaming_audio_mode",
        value as StreamingAudioModeValue,
      );
    } catch (error) {
      console.error("Failed to update streaming audio mode:", error);
    }
  };

  return (
    <SettingContainer
      title={t("settings.debug.streamingAudioMode.title")}
      description={t("settings.debug.streamingAudioMode.description")}
      descriptionMode={descriptionMode}
      grouped={grouped}
      layout="horizontal"
    >
      <Dropdown
        options={options}
        selectedValue={currentMode}
        onSelect={handleSelect}
        disabled={!settings || isUpdating("streaming_audio_mode")}
      />
    </SettingContainer>
  );
};

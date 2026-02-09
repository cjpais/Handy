import React from "react";
import { useTranslation } from "react-i18next";
import { Dropdown } from "../ui/Dropdown";
import { SettingContainer } from "../ui/SettingContainer";
import { useSettings } from "../../hooks/useSettings";
import type { WhisperComputeMode } from "@/bindings";

interface WhisperComputeModeSettingProps {
  descriptionMode?: "tooltip" | "inline";
  grouped?: boolean;
}

export const WhisperComputeModeSetting: React.FC<
  WhisperComputeModeSettingProps
> = ({ descriptionMode = "inline", grouped = false }) => {
  const { t } = useTranslation();
  const { getSetting, updateSetting } = useSettings();

  const currentValue = (getSetting("whisper_compute_mode") ??
    "auto") as WhisperComputeMode;

  const options = [
    {
      value: "auto" as WhisperComputeMode,
      label: t("settings.advanced.whisperCompute.options.auto"),
    },
    {
      value: "gpu" as WhisperComputeMode,
      label: t("settings.advanced.whisperCompute.options.gpu"),
    },
    {
      value: "cpu" as WhisperComputeMode,
      label: t("settings.advanced.whisperCompute.options.cpu"),
    },
  ];

  return (
    <SettingContainer
      title={t("settings.advanced.whisperCompute.title")}
      description={t("settings.advanced.whisperCompute.description")}
      descriptionMode={descriptionMode}
      grouped={grouped}
    >
      <Dropdown
        options={options}
        selectedValue={currentValue}
        onSelect={(value) =>
          updateSetting("whisper_compute_mode", value as WhisperComputeMode)
        }
        disabled={false}
      />
    </SettingContainer>
  );
};

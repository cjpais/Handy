import React from "react";
import { useTranslation } from "react-i18next";
import { Dropdown } from "../ui/Dropdown";
import { SettingContainer } from "../ui/SettingContainer";
import { useSettings } from "../../hooks/useSettings";
import type { TranscriptionMode } from "@/bindings";

interface TranscriptionModeSettingProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const TranscriptionModeSetting: React.FC<TranscriptionModeSettingProps> =
  React.memo(({ descriptionMode = "tooltip", grouped = false }) => {
    const { t } = useTranslation();
    const { getSetting, updateSetting, isUpdating } = useSettings();

    const selectedMode = (getSetting("transcription_mode") ||
      "standard") as TranscriptionMode;

    const options = [
      {
        value: "standard",
        label: t("settings.experimental.transcriptionMode.options.standard"),
      },
      {
        value: "realtime",
        label: t("settings.experimental.transcriptionMode.options.realtime"),
      },
      {
        value: "stream",
        label: t("settings.experimental.transcriptionMode.options.stream"),
      },
      {
        value: "batch_stream",
        label: t("settings.experimental.transcriptionMode.options.batchStream"),
      },
    ];

    return (
      <SettingContainer
        title={t("settings.experimental.transcriptionMode.title")}
        description={t("settings.experimental.transcriptionMode.description")}
        descriptionMode={descriptionMode}
        grouped={grouped}
        tooltipPosition="bottom"
      >
        <Dropdown
          options={options}
          selectedValue={selectedMode}
          onSelect={(value) =>
            updateSetting("transcription_mode", value as TranscriptionMode)
          }
          disabled={isUpdating("transcription_mode")}
        />
      </SettingContainer>
    );
  });

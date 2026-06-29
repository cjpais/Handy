import React from "react";
import { useTranslation } from "react-i18next";
import type { TranscriptionMode } from "@/bindings";
import { useSettings } from "../../hooks/useSettings";
import { Dropdown } from "../ui/Dropdown";
import { SettingContainer } from "../ui/SettingContainer";

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

    return (
      <SettingContainer
        title={t("settings.advanced.transcriptionMode.title")}
        description={t("settings.advanced.transcriptionMode.description")}
        descriptionMode={descriptionMode}
        grouped={grouped}
      >
        <Dropdown
          options={[
            {
              value: "standard",
              label: t(
                "settings.advanced.transcriptionMode.options.standard",
              ),
            },
            {
              value: "vad_chunked",
              label: t(
                "settings.advanced.transcriptionMode.options.vadChunked",
              ),
            },
          ]}
          selectedValue={selectedMode}
          onSelect={(value) =>
            updateSetting("transcription_mode", value as TranscriptionMode)
          }
          disabled={isUpdating("transcription_mode")}
        />
      </SettingContainer>
    );
  });

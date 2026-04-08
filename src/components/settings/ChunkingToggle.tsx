import React from "react";
import { useTranslation } from "react-i18next";
import { ToggleSwitch } from "../ui/ToggleSwitch";
import { useSettings } from "../../hooks/useSettings";

interface ChunkingToggleProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const ChunkingToggle: React.FC<ChunkingToggleProps> = React.memo(
  ({ descriptionMode = "tooltip", grouped = false }) => {
    const { t } = useTranslation();
    const { getSetting, updateSetting, isUpdating } = useSettings();

    const enabled = getSetting("chunked_transcription_enabled");

    return (
      <ToggleSwitch
        checked={enabled ?? false}
        onChange={(value) => updateSetting("chunked_transcription_enabled", value)}
        isUpdating={isUpdating("chunked_transcription_enabled")}
        label={t("settings.advanced.chunkingToggle.label")}
        description={t("settings.advanced.chunkingToggle.description")}
        descriptionMode={descriptionMode}
        grouped={grouped}
      />
    );
  },
);

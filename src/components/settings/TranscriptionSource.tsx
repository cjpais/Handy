import React from "react";
import { Dropdown } from "../ui/Dropdown";
import { SettingContainer } from "../ui/SettingContainer";
import { useSettings } from "../../hooks/useSettings";
import type { TranscriptionSource } from "../../lib/types";

interface TranscriptionSourceProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

const transcriptionSourceOptions = [
  { value: "local", label: "Local Model" },
  { value: "api", label: "API (Gemini)" },
];

export const TranscriptionSourceSetting: React.FC<TranscriptionSourceProps> = React.memo(
  ({ descriptionMode = "tooltip", grouped = false }) => {
    const { getSetting, updateSetting, isUpdating } = useSettings();

    const selectedSource = (getSetting("transcription_source") ||
      "local") as TranscriptionSource;

    return (
      <SettingContainer
        title="Transcription Source"
        description="Choose between local Whisper/Parakeet models or cloud-based API (Gemini). API requires an API key and internet connection but may offer faster processing."
        descriptionMode={descriptionMode}
        grouped={grouped}
        tooltipPosition="bottom"
      >
        <Dropdown
          options={transcriptionSourceOptions}
          selectedValue={selectedSource}
          onSelect={(value) =>
            updateSetting("transcription_source", value as TranscriptionSource)
          }
          disabled={isUpdating("transcription_source")}
        />
      </SettingContainer>
    );
  },
);

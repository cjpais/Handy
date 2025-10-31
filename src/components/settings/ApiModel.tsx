import React from "react";
import { Dropdown } from "../ui/Dropdown";
import { SettingContainer } from "../ui/SettingContainer";
import { useSettings } from "../../hooks/useSettings";

interface ApiModelProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

const apiModelOptions = [
  { value: "gemini-2.0-flash", label: "Gemini 2.0 Flash" },
  { value: "gemini-2.5-flash", label: "Gemini 2.5 Flash" },
  { value: "gemini-1.5-flash", label: "Gemini 1.5 Flash" },
  { value: "gemini-1.5-pro", label: "Gemini 1.5 Pro" },
];

export const ApiModelSetting: React.FC<ApiModelProps> = React.memo(
  ({ descriptionMode = "tooltip", grouped = false }) => {
    const { getSetting, updateSetting, isUpdating } = useSettings();

    const selectedModel = (getSetting("api_model") || "gemini-2.0-flash") as string;

    return (
      <SettingContainer
        title="API Model"
        description="Choose which Gemini model to use for transcription. Flash models are faster and cheaper, Pro models offer higher quality."
        descriptionMode={descriptionMode}
        grouped={grouped}
        tooltipPosition="bottom"
      >
        <Dropdown
          options={apiModelOptions}
          selectedValue={selectedModel}
          onSelect={(value) => updateSetting("api_model", value)}
          disabled={isUpdating("api_model")}
        />
      </SettingContainer>
    );
  },
);

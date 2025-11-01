import React from "react";
import { useSettings } from "../../hooks/useSettings";
import { Input } from "../ui/Input";
import { SettingContainer } from "../ui/SettingContainer";

interface ApiEndpointProps {
  descriptionMode?: "tooltip" | "inline";
  grouped?: boolean;
}

export const ApiEndpointSetting: React.FC<ApiEndpointProps> = ({
  descriptionMode = "tooltip",
  grouped = false,
}) => {
  const { getSetting, updateSetting, isUpdating } = useSettings();

  const apiEndpoint = getSetting("api_endpoint") ?? "https://generativelanguage.googleapis.com/v1beta/openai/";

  const handleChange = async (event: React.ChangeEvent<HTMLInputElement>) => {
    const value = event.target.value;
    updateSetting("api_endpoint", value);
  };

  return (
    <SettingContainer
      title="API Endpoint"
      description="The base URL for the Gemini API. Default uses the OpenAI-compatible endpoint. Advanced users only."
      descriptionMode={descriptionMode}
      grouped={grouped}
      layout="stacked"
    >
      <Input
        type="text"
        placeholder="https://generativelanguage.googleapis.com/v1beta/openai/"
        value={apiEndpoint}
        onChange={handleChange}
        disabled={isUpdating("api_endpoint")}
        className="w-full font-mono text-xs"
      />
    </SettingContainer>
  );
};

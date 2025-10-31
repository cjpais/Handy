import React from "react";
import { useSettings } from "../../hooks/useSettings";
import { Input } from "../ui/Input";
import { SettingContainer } from "../ui/SettingContainer";

interface ApiKeyProps {
  descriptionMode?: "tooltip" | "inline";
  grouped?: boolean;
}

export const ApiKeySetting: React.FC<ApiKeyProps> = ({
  descriptionMode = "tooltip",
  grouped = false,
}) => {
  const { getSetting, updateSetting, isUpdating } = useSettings();

  const apiKey = getSetting("api_key") ?? "";

  const handleChange = async (event: React.ChangeEvent<HTMLInputElement>) => {
    const value = event.target.value;
    updateSetting("api_key", value);
  };

  return (
    <SettingContainer
      title="API Key"
      description="Enter your Gemini API key. Get one from Google AI Studio (ai.google.dev). Keep this key secure and never share it."
      descriptionMode={descriptionMode}
      grouped={grouped}
      layout="stacked"
    >
      <Input
        type="password"
        placeholder="Enter your Gemini API key"
        value={apiKey}
        onChange={handleChange}
        disabled={isUpdating("api_key")}
        className="w-full font-mono"
      />
    </SettingContainer>
  );
};

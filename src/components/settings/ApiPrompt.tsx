import React from "react";
import { useSettings } from "../../hooks/useSettings";
import { Textarea } from "../ui/Textarea";
import { SettingContainer } from "../ui/SettingContainer";

interface ApiPromptProps {
  descriptionMode?: "tooltip" | "inline";
  grouped?: boolean;
}

export const ApiPromptSetting: React.FC<ApiPromptProps> = ({
  descriptionMode = "tooltip",
  grouped = false,
}) => {
  const { getSetting, updateSetting, isUpdating } = useSettings();

  const apiPrompt = getSetting("api_prompt") ?? "Transcribe this audio. Return only the transcribed text without any additional commentary.";

  const handleChange = async (event: React.ChangeEvent<HTMLTextAreaElement>) => {
    const value = event.target.value;
    updateSetting("api_prompt", value);
  };

  return (
    <SettingContainer
      title="API Prompt"
      description="Customize the prompt sent to the API with your audio. Use this to add instructions for formatting, language hints, or specific transcription requirements."
      descriptionMode={descriptionMode}
      grouped={grouped}
      layout="stacked"
    >
      <Textarea
        placeholder="Transcribe this audio. Return only the transcribed text without any additional commentary."
        value={apiPrompt}
        onChange={handleChange}
        disabled={isUpdating("api_prompt")}
        className="w-full font-mono text-xs min-h-[80px]"
        rows={3}
      />
    </SettingContainer>
  );
};

import React, { useState, useEffect } from "react";
import { SettingContainer } from "../ui/SettingContainer";
import { ResetButton } from "../ui/ResetButton";
import { useSettings } from "../../hooks/useSettings";

interface InitialPromptProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
  disabled?: boolean;
}

export const InitialPrompt: React.FC<InitialPromptProps> = React.memo(({
  descriptionMode = "tooltip",
  grouped = false,
  disabled = false,
}) => {
  const { getSetting, updateSetting, resetSetting, isUpdating } = useSettings();

  const settingValue = getSetting("initial_prompt") || "";
  const [localValue, setLocalValue] = useState(settingValue);

  // Sync local value with setting value when it changes externally
  useEffect(() => {
    setLocalValue(settingValue);
  }, [settingValue]);

  const handleChange = (event: React.ChangeEvent<HTMLTextAreaElement>) => {
    setLocalValue(event.target.value);
  };

  const handleBlur = () => {
    if (localValue !== settingValue) {
      updateSetting("initial_prompt", localValue);
    }
  };

  const handleReset = () => {
    resetSetting("initial_prompt");
  };

  return (
    <SettingContainer
      title="Whisper Initial Prompt"
      description="Provide context or formatting hints to improve transcription quality"
      descriptionMode={descriptionMode}
      grouped={grouped}
      disabled={disabled}
    >
      <div className="flex items-start gap-2">
        <textarea
          value={localValue}
          onChange={handleChange}
          onBlur={handleBlur}
          disabled={disabled || isUpdating("initial_prompt")}
          placeholder="eg. The following is an audio clip about programming. Please prioritize using industry-specific terminology."
          className="flex-1 px-3 py-2 text-sm border border-gray-300 dark:border-gray-600 rounded-md 
                     bg-white dark:bg-gray-800 text-gray-900 dark:text-gray-100
                     focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent
                     disabled:opacity-50 disabled:cursor-not-allowed
                     resize-vertical min-h-[80px] max-h-[200px]"
          rows={3}
        />
        <ResetButton
          onClick={handleReset}
          disabled={disabled || isUpdating("initial_prompt")}
        />
      </div>
    </SettingContainer>
  );
});

InitialPrompt.displayName = "InitialPrompt";
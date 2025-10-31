import React, { useState } from "react";
import { useSettings } from "../../hooks/useSettings";
import { SettingRow } from "../ui/SettingRow";

interface GeminiApiKeyProps {
  descriptionMode?: "tooltip" | "text";
  grouped?: boolean;
}

export const GeminiApiKey: React.FC<GeminiApiKeyProps> = ({
  descriptionMode = "text",
  grouped = false,
}) => {
  const { settings, updateSettings } = useSettings();
  const [showKey, setShowKey] = useState(false);
  const [localKey, setLocalKey] = useState(settings?.gemini_api_key || "");

  const handleSave = () => {
    if (settings) {
      updateSettings({
        ...settings,
        gemini_api_key: localKey || null,
      });
    }
  };

  const handleKeyChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    setLocalKey(e.target.value);
  };

  const handleKeyBlur = () => {
    handleSave();
  };

  const handleKeyPress = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (e.key === "Enter") {
      handleSave();
    }
  };

  return (
    <SettingRow
      title="Gemini API Key"
      description="Enter your Google AI API key to use Gemini models. Get one at ai.google.dev"
      descriptionMode={descriptionMode}
      grouped={grouped}
    >
      <div className="flex items-center gap-2 w-full">
        <div className="relative flex-1">
          <input
            type={showKey ? "text" : "password"}
            value={localKey}
            onChange={handleKeyChange}
            onBlur={handleKeyBlur}
            onKeyPress={handleKeyPress}
            placeholder="Enter API key..."
            className="w-full px-3 py-2 bg-white dark:bg-gray-800 border border-gray-300 dark:border-gray-600 rounded-lg text-sm focus:outline-none focus:ring-2 focus:ring-blue-500 dark:text-white pr-10"
          />
          <button
            onClick={() => setShowKey(!showKey)}
            className="absolute right-2 top-1/2 -translate-y-1/2 text-gray-500 hover:text-gray-700 dark:text-gray-400 dark:hover:text-gray-200 text-sm"
            type="button"
          >
            {showKey ? "Hide" : "Show"}
          </button>
        </div>
      </div>
    </SettingRow>
  );
};

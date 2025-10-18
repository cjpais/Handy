import React, { useState } from "react";
import { useSettings } from "../../hooks/useSettings";
import { Input } from "../ui/Input";
import { Button } from "../ui/Button";
import { SettingContainer } from "../ui/SettingContainer";

export const OpenRouterConfiguration: React.FC = React.memo(() => {
  const { getSetting, updateSetting, isUpdating } = useSettings();
  
  // States for edit mode
  const [isEditingApiKey, setIsEditingApiKey] = useState(false);
  const [isEditingModel, setIsEditingModel] = useState(false);
  const [tempApiKey, setTempApiKey] = useState("");
  const [tempModel, setTempModel] = useState("");

  const enabled = getSetting("post_process_enabled") || false;
  const apiKey = getSetting("post_process_api_key") || "";
  const model = getSetting("post_process_model") || "";

  const handleStartEditApiKey = () => {
    setTempApiKey(apiKey);
    setIsEditingApiKey(true);
  };

  const handleSaveApiKey = () => {
    updateSetting("post_process_api_key", tempApiKey);
    setIsEditingApiKey(false);
  };

  const handleStartEditModel = () => {
    setTempModel(model);
    setIsEditingModel(true);
  };

  const handleSaveModel = () => {
    updateSetting("post_process_model", tempModel);
    setIsEditingModel(false);
  };

  if (!enabled) {
    return (
      <div className="p-4 bg-mid-gray/5 rounded-lg border border-mid-gray/20 text-center">
        <p className="text-sm text-mid-gray">
          Post processing is currently disabled. Enable it in Debug settings to configure.
        </p>
      </div>
    );
  }

  return (
    <>
      <SettingContainer
        title="OpenRouter API Key"
        description="Your OpenRouter API key for accessing LLM models."
        descriptionMode="tooltip"
        layout="stacked"
        grouped={true}
      >
        <div className="flex items-center gap-2">
          <div className="flex-1 relative flex items-center gap-10">
            <Input
              type={isEditingApiKey ? "text" : "password"}
              value={isEditingApiKey ? tempApiKey : apiKey}
              onChange={(e) => setTempApiKey(e.target.value)}
              placeholder="sk-or-v1-..."
              variant="compact"
              disabled={!isEditingApiKey || isUpdating("post_process_api_key")}
              className="flex-grow"
            />
          </div>
          {!isEditingApiKey ? (
            <Button
              onClick={handleStartEditApiKey}
              variant="secondary"
              size="md"
            >
              Edit
            </Button>
          ) : (
            <Button
              onClick={handleSaveApiKey}
              variant="primary"
              size="md"
              disabled={!tempApiKey.trim()}
            >
              Save
            </Button>
          )}
        </div>
      </SettingContainer>

      <SettingContainer
        title="OpenRouter Model"
        description="The OpenRouter model to use (e.g., google/gemini-2.0-flash, openai/gpt-oss-20b, openai/gpt-5-mini)."
        descriptionMode="tooltip"
        layout="stacked"
        grouped={true}
      >
        <div className="flex items-center gap-2">
          <Input
            type="text"
            value={isEditingModel ? tempModel : model}
            onChange={(e) => setTempModel(e.target.value)}
            placeholder="openai/gpt-5-mini"
            variant="compact"
            disabled={!isEditingModel || isUpdating("post_process_model")}
            className="flex-1"
          />
          {!isEditingModel ? (
            <Button
              onClick={handleStartEditModel}
              variant="secondary"
              size="md"
            >
              Edit
            </Button>
          ) : (
            <Button
              onClick={handleSaveModel}
              variant="primary"
              size="md"
              disabled={!tempModel.trim()}
            >
              Save
            </Button>
          )}
        </div>
      </SettingContainer>
    </>
  );
});

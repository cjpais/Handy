import React, { useState } from "react";
import { useSettings } from "../../hooks/useSettings";
import { Input } from "../ui/Input";
import { Button } from "../ui/Button";
import { SettingContainer } from "../ui/SettingContainer";

export const PostProcessingSettingsApi: React.FC = React.memo(() => {
  const { getSetting, updateSetting, isUpdating } = useSettings();
  
  // States for edit mode
  const [isEditingBaseUrl, setIsEditingBaseUrl] = useState(false);
  const [isEditingApiKey, setIsEditingApiKey] = useState(false);
  const [isEditingModel, setIsEditingModel] = useState(false);
  const [tempBaseUrl, setTempBaseUrl] = useState("");
  const [tempApiKey, setTempApiKey] = useState("");
  const [tempModel, setTempModel] = useState("");

  const enabled = getSetting("post_process_enabled") || false;
  const baseUrl = getSetting("post_process_base_url") || "";
  const apiKey = getSetting("post_process_api_key") || "";
  const model = getSetting("post_process_model") || "";

  const handleStartEditBaseUrl = () => {
    setTempBaseUrl(baseUrl);
    setIsEditingBaseUrl(true);
  };

  const handleSaveBaseUrl = () => {
    updateSetting("post_process_base_url", tempBaseUrl);
    setIsEditingBaseUrl(false);
  };

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
        title="Base URL"
        description="OpenAI-compatible API base URL (e.g., https://api.openai.com/v1 for OpenAI, https://openrouter.ai/api/v1 for OpenRouter, http://localhost/v1 for local LLM)."
        descriptionMode="tooltip"
        layout="stacked"
        grouped={true}
      >
        <div className="flex items-center gap-2">
          <Input
            type="text"
            value={isEditingBaseUrl ? tempBaseUrl : baseUrl}
            onChange={(e) => setTempBaseUrl(e.target.value)}
            placeholder="https://api.openai.com/v1"
            variant="compact"
            disabled={!isEditingBaseUrl || isUpdating("post_process_base_url")}
            className="flex-1"
          />
          {!isEditingBaseUrl ? (
            <Button
              onClick={handleStartEditBaseUrl}
              variant="secondary"
              size="md"
            >
              Edit
            </Button>
          ) : (
            <Button
              onClick={handleSaveBaseUrl}
              variant="primary"
              size="md"
              disabled={!tempBaseUrl.trim()}
            >
              Save
            </Button>
          )}
        </div>
      </SettingContainer>

      <SettingContainer
        title="API Key"
        description="Your API key for the OpenAI-compatible endpoint."
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
              placeholder="sk-..."
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
        title="Model"
        description="The model to use (e.g., gpt-4, gpt-5 for OpenAI, or provider/model for OpenRouter)."
        descriptionMode="tooltip"
        layout="stacked"
        grouped={true}
      >
        <div className="flex items-center gap-2">
          <Input
            type="text"
            value={isEditingModel ? tempModel : model}
            onChange={(e) => setTempModel(e.target.value)}
            placeholder="gpt-3.5-turbo"
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

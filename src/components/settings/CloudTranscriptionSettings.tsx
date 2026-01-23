import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { SettingContainer } from "../ui/SettingContainer";
import { SettingsGroup } from "../ui/SettingsGroup";
import { Dropdown } from "../ui/Dropdown";
import { Input } from "../ui/Input";
import { Alert } from "../ui/Alert";
import { useSettingsStore } from "../../stores/settingsStore";

interface CloudTranscriptionSettingsProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

const CloudTranscriptionSettingsComponent: React.FC<
  CloudTranscriptionSettingsProps
> = ({ descriptionMode = "tooltip", grouped = false }) => {
  const { t } = useTranslation();
  const {
    settings,
    isUpdatingKey,
    setTranscriptionMode,
    setCloudTranscriptionProvider,
    updateCloudTranscriptionApiKey,
    updateCloudTranscriptionModel,
  } = useSettingsStore();

  const [apiKeyInput, setApiKeyInput] = useState("");

  const transcriptionMode = settings?.transcription_mode ?? "local";
  const isCloudMode = transcriptionMode === "cloud";
  // Use first provider as fallback instead of hardcoded "groq"
  const selectedProviderId =
    settings?.cloud_transcription_provider_id ??
    settings?.cloud_transcription_providers?.[0]?.id ??
    "";
  const selectedProvider = settings?.cloud_transcription_providers?.find(
    (p) => p.id === selectedProviderId,
  );
  const currentApiKey =
    settings?.cloud_transcription_api_keys?.[selectedProviderId] ?? "";
  // Use provider's default_model as fallback instead of hardcoded value
  const currentModel =
    settings?.cloud_transcription_models?.[selectedProviderId] ??
    selectedProvider?.default_model ??
    "";

  // Get models directly from provider definition (no backend call needed)
  const availableModels = selectedProvider?.models ?? [];

  // Sync API key input with stored value
  useEffect(() => {
    setApiKeyInput(currentApiKey);
  }, [currentApiKey]);

  const modeOptions = [
    {
      value: "local",
      label: t("settings.cloudTranscription.mode.local"),
    },
    {
      value: "cloud",
      label: t("settings.cloudTranscription.mode.cloud"),
    },
  ];

  const providerOptions =
    settings?.cloud_transcription_providers?.map((provider) => ({
      value: provider.id,
      label: provider.label,
    })) ?? [];

  const modelOptions = availableModels.map((model) => ({
    value: model,
    label: model,
  }));

  const handleApiKeyBlur = () => {
    if (apiKeyInput !== currentApiKey) {
      updateCloudTranscriptionApiKey(selectedProviderId, apiKeyInput);
    }
  };

  return (
    <SettingsGroup title={t("settings.cloudTranscription.title")}>
      <SettingContainer
        title={t("settings.cloudTranscription.mode.title")}
        description={t("settings.cloudTranscription.mode.description")}
        descriptionMode={descriptionMode}
        grouped={grouped}
      >
        <Dropdown
          options={modeOptions}
          selectedValue={transcriptionMode}
          onSelect={(value) => setTranscriptionMode(value)}
          disabled={isUpdatingKey("transcription_mode")}
        />
      </SettingContainer>

      {isCloudMode && (
        <>
          <SettingContainer
            title={t("settings.cloudTranscription.provider.title")}
            description={t("settings.cloudTranscription.provider.description")}
            descriptionMode={descriptionMode}
            grouped={true}
          >
            <Dropdown
              options={providerOptions}
              selectedValue={selectedProviderId}
              onSelect={(value) => setCloudTranscriptionProvider(value)}
              disabled={isUpdatingKey("cloud_transcription_provider_id")}
            />
          </SettingContainer>

          <SettingContainer
            title={t("settings.cloudTranscription.apiKey.title")}
            description={t("settings.cloudTranscription.apiKey.description")}
            descriptionMode={descriptionMode}
            grouped={true}
          >
            <Input
              type="password"
              value={apiKeyInput}
              onChange={(e) => setApiKeyInput(e.target.value)}
              onBlur={handleApiKeyBlur}
              placeholder={
                selectedProvider?.api_key_placeholder ??
                t("settings.cloudTranscription.apiKey.placeholder")
              }
              disabled={isUpdatingKey(
                `cloud_transcription_api_key:${selectedProviderId}`,
              )}
              className="min-w-[200px]"
            />
          </SettingContainer>

          <SettingContainer
            title={t("settings.cloudTranscription.model.title")}
            description={t("settings.cloudTranscription.model.description")}
            descriptionMode={descriptionMode}
            grouped={true}
          >
            <Dropdown
              options={modelOptions}
              selectedValue={currentModel}
              onSelect={(value) =>
                updateCloudTranscriptionModel(selectedProviderId, value)
              }
              disabled={isUpdatingKey(
                `cloud_transcription_model:${selectedProviderId}`,
              )}
            />
          </SettingContainer>
        </>
      )}

      {isCloudMode && !currentApiKey && (
        <Alert variant="warning" className="mt-2">
          {t("settings.cloudTranscription.apiKey.missing")}
        </Alert>
      )}

      {isCloudMode && selectedProvider && currentApiKey && (
        <div className="text-xs text-mid-gray/70 px-1 mt-2">
          {t("settings.cloudTranscription.providerNote", {
            provider: selectedProvider.label,
          })}
        </div>
      )}
    </SettingsGroup>
  );
};

export const CloudTranscriptionSettings = React.memo(
  CloudTranscriptionSettingsComponent,
);
CloudTranscriptionSettings.displayName = "CloudTranscriptionSettings";

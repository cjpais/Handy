import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { useSettings } from "../../hooks/useSettings";
import { Input } from "../ui/Input";
import { SettingContainer } from "../ui/SettingContainer";

interface CustomTranscriptionEndpointProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const CustomTranscriptionEndpoint: React.FC<CustomTranscriptionEndpointProps> =
  React.memo(({ descriptionMode = "tooltip", grouped = false }) => {
    const { t } = useTranslation();
    const { getSetting, updateSetting, isUpdating } = useSettings();

    const endpoint = getSetting("custom_transcription_endpoint") || "";
    const model = getSetting("custom_transcription_model") || "whisper-1";
    const [endpointValue, setEndpointValue] = useState(endpoint);
    const [modelValue, setModelValue] = useState(model);

    useEffect(() => {
      setEndpointValue(endpoint);
    }, [endpoint]);

    useEffect(() => {
      setModelValue(model);
    }, [model]);

    useEffect(() => {
      const timeout = window.setTimeout(() => {
        if (endpointValue !== endpoint) {
          updateSetting("custom_transcription_endpoint", endpointValue || null);
        }
      }, 400);

      return () => window.clearTimeout(timeout);
    }, [endpointValue, endpoint, updateSetting]);

    useEffect(() => {
      const timeout = window.setTimeout(() => {
        if (modelValue !== model) {
          updateSetting("custom_transcription_model", modelValue);
        }
      }, 400);

      return () => window.clearTimeout(timeout);
    }, [modelValue, model, updateSetting]);

    return (
      <SettingContainer
        title={t("settings.advanced.customTranscription.title")}
        description={t("settings.advanced.customTranscription.description")}
        descriptionMode={descriptionMode}
        grouped={grouped}
        layout="stacked"
      >
        <div className="grid grid-cols-1 sm:grid-cols-[1fr_10rem] gap-2">
          <Input
            type="url"
            value={endpointValue}
            onChange={(event) => setEndpointValue(event.target.value)}
            placeholder={t(
              "settings.advanced.customTranscription.endpointPlaceholder",
            )}
            aria-busy={isUpdating("custom_transcription_endpoint")}
          />
          <Input
            type="text"
            value={modelValue}
            onChange={(event) => setModelValue(event.target.value)}
            placeholder={t(
              "settings.advanced.customTranscription.modelPlaceholder",
            )}
            aria-busy={isUpdating("custom_transcription_model")}
          />
        </div>
      </SettingContainer>
    );
  });

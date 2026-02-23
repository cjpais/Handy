import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { commands } from "@/bindings";

export const CloudModelConfig: React.FC = () => {
  const { t } = useTranslation();
  const [baseUrl, setBaseUrl] = useState("");
  const [apiKey, setApiKey] = useState("");
  const [model, setModel] = useState("");

  // Load initial values from settings
  useEffect(() => {
    commands.getAppSettings().then((result) => {
      if (result.status === "ok") {
        const s = result.data;
        setBaseUrl(
          s.cloud_transcription_base_url ?? "https://api.groq.com/openai/v1",
        );
        setApiKey(s.cloud_transcription_api_key ?? "");
        setModel(s.cloud_transcription_model ?? "whisper-large-v3");
      }
    });
  }, []);

  const saveBaseUrl = (value: string) => {
    commands.changeCloudTranscriptionBaseUrl(value).catch(console.error);
  };

  const saveApiKey = (value: string) => {
    commands.changeCloudTranscriptionApiKey(value).catch(console.error);
  };

  const saveModel = (value: string) => {
    commands.changeCloudTranscriptionModel(value).catch(console.error);
  };

  return (
    <div className="mt-3 space-y-2 p-3 bg-mid-gray/5 rounded-lg border border-mid-gray/20">
      <div className="flex flex-col gap-1">
        <label className="text-xs font-medium text-text/60">
          {t("settings.models.cloudTranscription.baseUrlLabel")}
        </label>
        <input
          type="text"
          value={baseUrl}
          onChange={(e) => setBaseUrl(e.target.value)}
          onBlur={(e) => saveBaseUrl(e.target.value)}
          placeholder={t(
            "settings.models.cloudTranscription.baseUrlPlaceholder",
          )}
          className="w-full px-3 py-1.5 text-sm bg-background border border-mid-gray/40 rounded-md focus:outline-none focus:ring-1 focus:ring-logo-primary"
        />
      </div>
      <div className="flex flex-col gap-1">
        <label className="text-xs font-medium text-text/60">
          {t("settings.models.cloudTranscription.apiKeyLabel")}
        </label>
        <input
          type="password"
          value={apiKey}
          onChange={(e) => setApiKey(e.target.value)}
          onBlur={(e) => saveApiKey(e.target.value)}
          placeholder={t(
            "settings.models.cloudTranscription.apiKeyPlaceholder",
          )}
          className="w-full px-3 py-1.5 text-sm bg-background border border-mid-gray/40 rounded-md focus:outline-none focus:ring-1 focus:ring-logo-primary"
        />
      </div>
      <div className="flex flex-col gap-1">
        <label className="text-xs font-medium text-text/60">
          {t("settings.models.cloudTranscription.modelLabel")}
        </label>
        <input
          type="text"
          value={model}
          onChange={(e) => setModel(e.target.value)}
          onBlur={(e) => saveModel(e.target.value)}
          placeholder={t(
            "settings.models.cloudTranscription.modelPlaceholder",
          )}
          className="w-full px-3 py-1.5 text-sm bg-background border border-mid-gray/40 rounded-md focus:outline-none focus:ring-1 focus:ring-logo-primary"
        />
      </div>
    </div>
  );
};

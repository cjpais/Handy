import React from "react";
import { useTranslation } from "react-i18next";
import { SettingsGroup } from "../../ui/SettingsGroup";
import { LanguageSelector } from "../LanguageSelector";
import { TranslateToEnglish } from "../TranslateToEnglish";
import { OpenAiWhisperApiKey } from "../OpenAiWhisperApiKey";
import { useModelStore } from "../../../stores/modelStore";
import type { ModelInfo } from "@/bindings";

export const ModelSettingsCard: React.FC = () => {
  const { t } = useTranslation();
  const { currentModel, models } = useModelStore();

  const currentModelInfo = models.find((m: ModelInfo) => m.id === currentModel);

  const supportsLanguageSelection =
    currentModelInfo?.engine_type === "Whisper" ||
    currentModelInfo?.engine_type === "SenseVoice";
  const supportsTranslation = currentModelInfo?.supports_translation ?? false;
  const isOpenAI = currentModelInfo?.engine_type === "OpenAI";
  const hasAnySettings = supportsLanguageSelection || supportsTranslation || isOpenAI;

  // Don't render anything if no model is selected or no settings available
  if (!currentModel || !currentModelInfo || !hasAnySettings) {
    return null;
  }

  return (
    <>
      {isOpenAI && <OpenAiWhisperApiKey />}
      {supportsLanguageSelection && (
        <SettingsGroup
          title={t("settings.modelSettings.title", {
            model: currentModelInfo.name,
          })}
        >
          <LanguageSelector
            descriptionMode="tooltip"
            grouped={true}
            supportedLanguages={currentModelInfo.supported_languages}
          />
          {supportsTranslation && (
            <TranslateToEnglish descriptionMode="tooltip" grouped={true} />
          )}
        </SettingsGroup>
      )}
    </>
  );
};

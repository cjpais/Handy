import React from "react";
import { useTranslation } from "react-i18next";
import { SettingsGroup } from "../../ui/SettingsGroup";
import { LanguageSelector } from "../LanguageSelector";
import { TranslateToEnglish } from "../TranslateToEnglish";
import { useModelStore } from "../../../stores/modelStore";
import { useSettings } from "../../../hooks/useSettings";
import type { ModelInfo } from "@/bindings";
import { getActiveTranscriptionModelDisplayName } from "@/lib/utils/externalTranscriptionModel";

export const ModelSettingsCard: React.FC = () => {
  const { t } = useTranslation();
  const { currentModel, models } = useModelStore();
  const { getSetting } = useSettings();
  const transcriptionProviderId =
    getSetting("transcription_provider_id") || "local";
  const isElevenLabsSelected = transcriptionProviderId === "elevenlabs";

  const currentModelInfo = models.find((m: ModelInfo) => m.id === currentModel);

  if (isElevenLabsSelected) {
    return (
      <SettingsGroup
        title={t("settings.modelSettings.external.title", {
          defaultValue: "{{model}} Settings",
          model:
            getActiveTranscriptionModelDisplayName(transcriptionProviderId) ||
            "ElevenLabs",
        })}
      >
        <LanguageSelector descriptionMode="tooltip" grouped={true} />
      </SettingsGroup>
    );
  }

  const supportsLanguageSelection =
    currentModelInfo?.supports_language_selection ?? false;
  const supportsTranslation = currentModelInfo?.supports_translation ?? false;
  const hasAnySettings = supportsLanguageSelection || supportsTranslation;

  // Don't render anything if no model is selected or no settings available
  if (!currentModel || !currentModelInfo || !hasAnySettings) {
    return null;
  }

  return (
    <SettingsGroup
      title={t("settings.modelSettings.title", {
        model: currentModelInfo.name,
      })}
    >
      {supportsLanguageSelection && (
        <LanguageSelector
          descriptionMode="tooltip"
          grouped={true}
          supportedLanguages={currentModelInfo.supported_languages}
        />
      )}
      {supportsTranslation && (
        <TranslateToEnglish descriptionMode="tooltip" grouped={true} />
      )}
    </SettingsGroup>
  );
};

import React from "react";
import { useTranslation } from "react-i18next";
import { SettingsGroup } from "../../ui/SettingsGroup";
import { LanguageSelector } from "../LanguageSelector";
import { TranslateToEnglish } from "../TranslateToEnglish";
import { useModelStore } from "../../../stores/modelStore";
import type { ModelInfo } from "@/bindings";

export const ModelSettingsCard: React.FC = () => {
  const { t } = useTranslation();
  const { currentModel, models } = useModelStore();

  const currentModelInfo = models.find((m: ModelInfo) => m.id === currentModel);

  const supportsLanguageSelection = currentModelInfo?.engine_type === "Whisper";
  const supportsTranslation = currentModelInfo?.supports_translation ?? false;
  const hasAnySettings = Boolean(currentModelInfo);

  // Don't render anything if no model is selected or no settings available
  if (!currentModel || !currentModelInfo || !hasAnySettings) {
    return null;
  }

  const supportedLanguages = currentModelInfo.supported_languages ?? [];
  const languageOptions = supportsLanguageSelection
    ? ["auto", ...supportedLanguages]
    : ["auto"];
  const languageDescription = supportsLanguageSelection
    ? t("settings.general.language.description")
    : t("settings.general.language.descriptionUnsupported");

  return (
    <SettingsGroup
      title={t("settings.modelSettings.title", {
        model: currentModelInfo.name,
      })}
    >
      <LanguageSelector
        descriptionMode="tooltip"
        grouped={true}
        description={languageDescription}
        allowedLanguageCodes={languageOptions}
        disabled={!supportsLanguageSelection}
      />
      {supportsTranslation && (
        <TranslateToEnglish descriptionMode="tooltip" grouped={true} />
      )}
    </SettingsGroup>
  );
};

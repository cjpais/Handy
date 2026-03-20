import React from "react";
import { useTranslation } from "react-i18next";
import { SettingsGroup } from "../../ui/SettingsGroup";
import { LanguageSelector } from "../LanguageSelector";
import { TranslateToEnglish } from "../TranslateToEnglish";
import { useModelStore } from "../../../stores/modelStore";
import type { ModelInfo } from "@/bindings";

export const ModelSettingsCard: React.FC = () => {
  const { t } = useTranslation();
  const { currentModel, models, switchingModelId } = useModelStore();
  const modelSwitchInProgress = switchingModelId !== null;

  const currentModelInfo = models.find((m: ModelInfo) => m.id === currentModel);

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
        <>
          <LanguageSelector
            descriptionMode="tooltip"
            grouped={true}
            supportedLanguages={currentModelInfo.supported_languages}
            disabled={modelSwitchInProgress}
          />
          <LanguageSelector
            descriptionMode="tooltip"
            grouped={true}
            settingKey="secondary_selected_language"
            titleKey="settings.general.secondaryLanguage.title"
            descriptionKey="settings.general.secondaryLanguage.description"
            supportedLanguages={currentModelInfo.supported_languages}
            disabled={modelSwitchInProgress}
          />
        </>
      )}
      {supportsTranslation && (
        <TranslateToEnglish
          descriptionMode="tooltip"
          grouped={true}
          disabled={modelSwitchInProgress}
        />
      )}
    </SettingsGroup>
  );
};

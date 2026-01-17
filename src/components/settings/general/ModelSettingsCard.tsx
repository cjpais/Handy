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

  const supportsLanguage =
    currentModelInfo?.supports_language_selection ?? false;
  const supportsTranslation = currentModelInfo?.supports_translation ?? false;
  const hasAnySettings = supportsLanguage || supportsTranslation;

  // Don't render anything if no model is selected yet
  if (!currentModel || !currentModelInfo) {
    return null;
  }

  return (
    <SettingsGroup
      title={t("settings.modelSettings.title", {
        model: currentModelInfo.name,
      })}
    >
      {hasAnySettings ? (
        <>
          {supportsLanguage && (
            <LanguageSelector descriptionMode="tooltip" grouped={true} />
          )}
          {supportsTranslation && (
            <TranslateToEnglish descriptionMode="tooltip" grouped={true} />
          )}
        </>
      ) : (
        <div className="px-4 py-3 text-sm text-text/70">
          {t("settings.modelSettings.noSettingsNeeded")}
        </div>
      )}
    </SettingsGroup>
  );
};

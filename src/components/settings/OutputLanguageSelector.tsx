import React from "react";
import { useTranslation } from "react-i18next";
import { SettingContainer } from "../ui/SettingContainer";
import { useSettings } from "../../hooks/useSettings";
import type { OutputLanguage } from "@/bindings";

interface OutputLanguageSelectorProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const OutputLanguageSelector: React.FC<OutputLanguageSelectorProps> =
  React.memo(({ descriptionMode = "tooltip", grouped = false }) => {
    const { t } = useTranslation();
    const { getSetting, updateSetting, isUpdating } = useSettings();

    const selectedLanguage = (getSetting("output_language") ||
      "malayalam") as OutputLanguage;

    const languages: {
      value: OutputLanguage;
      label: string;
      description: string;
    }[] = [
      {
        value: "malayalam",
        label: t("settings.general.outputLanguage.options.malayalam.label"),
        description: t(
          "settings.general.outputLanguage.options.malayalam.description",
        ),
      },
      {
        value: "manglish",
        label: t("settings.general.outputLanguage.options.manglish.label"),
        description: t(
          "settings.general.outputLanguage.options.manglish.description",
        ),
      },
      {
        value: "english",
        label: t("settings.general.outputLanguage.options.english.label"),
        description: t(
          "settings.general.outputLanguage.options.english.description",
        ),
      },
    ];

    const handleChange = (value: OutputLanguage) => {
      if (isUpdating("output_language")) return;
      updateSetting("output_language", value);
    };

    return (
      <SettingContainer
        title={t("settings.general.outputLanguage.title")}
        description={t("settings.general.outputLanguage.description")}
        descriptionMode={descriptionMode}
        grouped={grouped}
      >
        <div className="flex bg-mid-gray/10 p-0.5 rounded-lg border border-mid-gray/20 w-full relative">
          {languages.map((lang) => {
            const isSelected = selectedLanguage === lang.value;
            return (
              <button
                key={lang.value}
                type="button"
                onClick={() => handleChange(lang.value)}
                disabled={isUpdating("output_language")}
                className={`flex-1 py-1.5 px-3 rounded-md text-xs font-medium transition-all duration-200 select-none relative z-10
                  ${
                    isSelected
                      ? "bg-logo-primary text-white shadow-sm"
                      : "text-mid-gray hover:text-text hover:bg-mid-gray/5"
                  }
                  ${isUpdating("output_language") ? "opacity-50 cursor-not-allowed" : ""}
                `}
                title={lang.description}
              >
                {lang.label}
              </button>
            );
          })}
        </div>
      </SettingContainer>
    );
  });

OutputLanguageSelector.displayName = "OutputLanguageSelector";

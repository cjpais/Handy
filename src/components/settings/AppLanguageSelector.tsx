import React from "react";
import { useTranslation } from "react-i18next";
import { Dropdown } from "../ui/Dropdown";
import { SettingContainer } from "../ui/SettingContainer";
import { SUPPORTED_LANGUAGES, type SupportedLanguageCode } from "../../i18n";

interface AppLanguageSelectorProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const AppLanguageSelector: React.FC<AppLanguageSelectorProps> =
  React.memo(({ descriptionMode = "tooltip", grouped = false }) => {
    const { t, i18n } = useTranslation();

    const currentLanguage = i18n.language as SupportedLanguageCode;

    const languageOptions = SUPPORTED_LANGUAGES.map((lang) => ({
      value: lang.code,
      label: `${lang.nativeName} (${lang.name})`,
    }));

    const handleLanguageChange = (langCode: string) => {
      i18n.changeLanguage(langCode);
      // Persist to localStorage for next session
      localStorage.setItem("handy-app-language", langCode);
    };

    return (
      <SettingContainer
        title={t("appLanguage.title")}
        description={t("appLanguage.description")}
        descriptionMode={descriptionMode}
        grouped={grouped}
      >
        <Dropdown
          options={languageOptions}
          selectedValue={currentLanguage}
          onSelect={handleLanguageChange}
        />
      </SettingContainer>
    );
  });

AppLanguageSelector.displayName = "AppLanguageSelector";

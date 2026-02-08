import React from "react";
import { useTranslation } from "react-i18next";
import { locale } from "@tauri-apps/plugin-os";
import { Dropdown } from "../ui/Dropdown";
import { SettingContainer } from "../ui/SettingContainer";
import { SUPPORTED_LANGUAGES, type SupportedLanguageCode } from "../../i18n";
import { useSettings } from "@/hooks/useSettings";

interface AppLanguageSelectorProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const AppLanguageSelector: React.FC<AppLanguageSelectorProps> =
  React.memo(({ descriptionMode = "tooltip", grouped = false }) => {
    const { t, i18n } = useTranslation();
    const { settings, updateSetting } = useSettings();

    const currentLanguage = (settings?.app_language ||
      i18n.language) as SupportedLanguageCode;

    const languageOptions = [
      { value: "auto", label: t("settings.general.language.auto") },
      ...SUPPORTED_LANGUAGES.map((lang) => ({
        value: lang.code,
        label: `${lang.nativeName} (${lang.name})`,
      })),
    ];

    const handleLanguageChange = async (langCode: string) => {
      if (langCode === "auto") {
        const systemLocale = await locale();
        const systemLang = systemLocale?.split("-")[0].toLowerCase() || "en";
        await i18n.changeLanguage(systemLang);
        await updateSetting("app_language", "auto");
        return;
      }

      await i18n.changeLanguage(langCode);
      await updateSetting("app_language", langCode);
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

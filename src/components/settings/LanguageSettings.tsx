import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { Check } from "lucide-react";
import i18n from "../../i18n";
import { SettingsGroup } from "../ui/SettingsGroup";
import {
  SUPPORTED_UI_LANGUAGES,
  UILanguage,
  UI_LANGUAGE_CHANGED_EVENT,
  getStoredUiLanguage,
  normalizeUiLanguage,
  setStoredUiLanguage,
} from "../../lib/constants/uiLanguage";

interface LanguageOption {
  language: UILanguage;
  label: string;
  description: string;
}

const LanguageSettings: React.FC = () => {
  const { t } = useTranslation();
  const [selectedLanguage, setSelectedLanguage] = useState<UILanguage>(() => {
    const stored = getStoredUiLanguage();
    if (stored) {
      return stored;
    }
    return normalizeUiLanguage(i18n.language);
  });
  const [isUpdatingLanguage, setIsUpdatingLanguage] = useState(false);

  useEffect(() => {
    if (typeof window === "undefined") {
      return;
    }

    const handleLanguageChanged = (event: Event) => {
      const detail = (event as CustomEvent<UILanguage>).detail;
      setSelectedLanguage(detail);
      setIsUpdatingLanguage(false);
    };

    window.addEventListener(
      UI_LANGUAGE_CHANGED_EVENT,
      handleLanguageChanged as EventListener,
    );

    return () => {
      window.removeEventListener(
        UI_LANGUAGE_CHANGED_EVENT,
        handleLanguageChanged as EventListener,
      );
    };
  }, []);

  const languageOptions: LanguageOption[] = useMemo(
    () =>
      SUPPORTED_UI_LANGUAGES.map((language) => ({
        language,
        label: t(`settings.languages.options.${language}.label`),
        description: t(`settings.languages.options.${language}.description`),
      })),
    [t],
  );

  const handleLanguageSelect = (language: UILanguage) => {
    if (language === selectedLanguage) {
      return;
    }

    setIsUpdatingLanguage(true);
    i18n.changeLanguage(language);
    setStoredUiLanguage(language);
    setSelectedLanguage(language);
  };

  return (
    <div className="max-w-3xl w-full mx-auto space-y-6">
      <SettingsGroup
        title={t("settings.languages.title")}
        description={t("settings.languages.description")}
      >
        <div className="flex flex-col gap-3 p-4">
          {languageOptions.map(({ language, label, description }) => {
            const isActive = selectedLanguage === language;

            return (
              <button
                key={language}
                type="button"
                className={`text-left border rounded-lg px-4 py-3 transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-logo-primary flex flex-col gap-1 ${
                  isActive
                    ? "border-logo-primary bg-logo-primary/10"
                    : "border-mid-gray/40 hover:border-logo-primary/80"
                } ${
                  isUpdatingLanguage ? "opacity-60 cursor-progress" : "hover:bg-mid-gray/10"
                }`}
                onClick={() => handleLanguageSelect(language)}
                disabled={isUpdatingLanguage}
                aria-pressed={isActive}
              >
                <div className="flex items-center justify-between gap-3">
                  <span className="text-base font-medium">{label}</span>
                  {isActive && <Check className="w-4 h-4 text-logo-primary" />}
                </div>
                <p className="text-sm text-mid-gray">{description}</p>
              </button>
            );
          })}
        </div>
      </SettingsGroup>
    </div>
  );
};

export default LanguageSettings;

import { useState } from "react";
import { useTranslation } from "react-i18next";
import HandyTextLogo from "../icons/HandyTextLogo";

type SupportedLanguage = "en" | "fr" | "es";

interface LanguageSetupProps {
  defaultLanguage: SupportedLanguage;
  onSelect: (language: SupportedLanguage) => void;
}

const AVAILABLE_LANGUAGES: SupportedLanguage[] = ["en", "fr", "es"];

const LanguageSetup: React.FC<LanguageSetupProps> = ({
  defaultLanguage,
  onSelect,
}) => {
  const { t } = useTranslation();
  const [selectedLanguage, setSelectedLanguage] = useState<SupportedLanguage>(
    defaultLanguage,
  );

  const handleConfirm = () => {
    onSelect(selectedLanguage);
  };

  return (
    <div className="h-screen w-screen flex flex-col items-center justify-center gap-8 p-6">
      <div className="flex flex-col items-center gap-2 text-center max-w-md">
        <HandyTextLogo width={220} />
        <h1 className="text-2xl font-semibold text-text">
          {t("language_setup.title")}
        </h1>
        <p className="text-text/70">
          {t("language_setup.description")}
        </p>
      </div>

      <div className="flex flex-col gap-4 w-full max-w-sm">
        {AVAILABLE_LANGUAGES.map((lang) => {
          const isActive = selectedLanguage === lang;
          return (
            <button
              key={lang}
              type="button"
              className={`flex items-center justify-between px-4 py-3 border rounded-lg transition-colors text-left ${
                isActive
                  ? "border-logo-primary bg-logo-primary/10"
                  : "border-mid-gray/50 hover:border-logo-primary"
              }`}
              onClick={() => setSelectedLanguage(lang)}
            >
              <span className="text-base font-medium">
                {t(`language_setup.languages.${lang}.label`)}
              </span>
              <span className="text-sm text-text/70">
                {t(`language_setup.languages.${lang}.description`)}
              </span>
            </button>
          );
        })}
      </div>

      <button
        type="button"
        className="px-6 py-2 bg-logo-primary text-background rounded-lg font-semibold hover:bg-logo-primary/90 transition-colors"
        onClick={handleConfirm}
      >
        {t("language_setup.confirm")}
      </button>
    </div>
  );
};

export default LanguageSetup;

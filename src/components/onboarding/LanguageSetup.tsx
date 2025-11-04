import React, { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Button } from '../ui/Button';
import { useSettings } from '../../hooks/useSettings';
import { i18n } from 'i18next';
import { Check, Globe } from 'lucide-react';

type SupportedLanguage = "en" | "fr" | "es";

interface LanguageSetupProps {
  defaultLanguage: SupportedLanguage;
  onSelect: (language: SupportedLanguage) => void;
}

const AVAILABLE_LANGUAGES: SupportedLanguage[] = ["en", "fr", "es"];

const languages = [
  { 
    code: "en", 
    label: "English", 
    description: "Select English as your preferred language",
    nativeName: "English"
  },
  { 
    code: "fr", 
    label: "Français", 
    description: "Sélectionnez le français comme langue préférée",
    nativeName: "Français"
  },
  { 
    code: "es", 
    label: "Español", 
    description: "Seleccione el español como idioma preferido",
    nativeName: "Español"
  },
];

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

  const handleLanguageSelect = (language: SupportedLanguage) => {
    setSelectedLanguage(language);
  };

  return (
    <div className="flex flex-col items-center justify-center min-h-screen p-6 bg-linear-to-b from-gray-50 to-gray-100 dark:from-gray-900 dark:to-gray-800">
      <div className="w-full max-w-md space-y-8">
        <div className="text-center">
          <div className="flex justify-center mb-4">
            <div className="p-3 rounded-full bg-blue-100 dark:bg-blue-900/50">
              <Globe className="w-8 h-8 text-blue-600 dark:text-blue-400" />
            </div>
          </div>
          <h1 className="text-3xl font-bold text-gray-900 dark:text-white">
            {t('language_setup.title', 'Bienvenue dans Handy')}
          </h1>
          <p className="mt-2 text-gray-600 dark:text-gray-400">
            {t('language_setup.description', 'Choisissez votre langue préférée pour continuer')}
          </p>
          <p className="mt-1 text-sm text-gray-500 dark:text-gray-400">
            {t('language_setup.can_change_later', 'Vous pourrez modifier ce choix ultérieurement dans les paramètres')}
          </p>
        </div>

        <div className="space-y-3">
          {languages.map((language) => (
            <button
              key={language.code}
              type="button"
              onClick={() => handleLanguageSelect(language.code as SupportedLanguage)}
              className={`w-full text-left p-4 border rounded-lg transition-all duration-200 flex items-start ${
                selectedLanguage === language.code
                  ? 'border-blue-500 bg-blue-50 dark:bg-blue-900/20 ring-2 ring-blue-500/30'
                  : 'border-gray-200 dark:border-gray-700 hover:bg-gray-50 dark:hover:bg-gray-800/50'
              }`}
            >
              <div className={`flex items-center justify-center w-5 h-5 mt-0.5 mr-3 rounded-full border ${
                selectedLanguage === language.code 
                  ? 'bg-blue-500 border-blue-500 text-white' 
                  : 'border-gray-300 dark:border-gray-600'
              }`}>
                {selectedLanguage === language.code && (
                  <Check className="w-3 h-3" />
                )}
              </div>
              <div>
                <h3 className="font-medium text-gray-900 dark:text-white">
                  {language.nativeName}
                </h3>
                <p className="mt-1 text-sm text-gray-500 dark:text-gray-400">
                  {language.description}
                </p>
              </div>
            </button>
          ))}
        </div>

        <Button
          onClick={handleConfirm}
          className="w-full py-2.5 text-base font-medium mt-4"
          size="lg"
        >
          {selectedLanguage === 'en' && "Continue"}
          {selectedLanguage === 'fr' && "Continuer"}
          {selectedLanguage === 'es' && "Continuar"}
        </Button>
      </div>
    </div>
  );
};

export default LanguageSetup;

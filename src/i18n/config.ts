import i18n from 'i18next';
import { initReactI18next } from 'react-i18next';
import LanguageDetector from 'i18next-browser-languagedetector';

// Import translation resources
import { resources } from './resources';

// Supported languages
export const supportedLanguages = {
  en: 'English',
  zh: '中文',
} as const;

export type SupportedLanguage = keyof typeof supportedLanguages;

// Default language
export const defaultLanguage: SupportedLanguage = 'en';

// i18n configuration
i18n
  .use(LanguageDetector) // Auto-detect user language
  .use(initReactI18next) // Pass i18n instance to react-i18next
  .init({
    resources,
    fallbackLng: defaultLanguage,
    debug: false, // Set to true for debugging

    // Language detection configuration
    detection: {
      order: ['localStorage', 'navigator', 'htmlTag'],
      caches: ['localStorage'],
    },

    interpolation: {
      escapeValue: false, // React already escapes values
    },

    // React options
    react: {
      useSuspense: false, // Disable suspense mode for simplicity
    },
  });

export default i18n;
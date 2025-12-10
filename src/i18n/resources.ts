import enTranslations from './locales/en.json';
import zhTranslations from './locales/zh.json';

export const resources = {
  en: {
    translation: enTranslations,
  },
  zh: {
    translation: zhTranslations,
  },
} as const;
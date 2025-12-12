/**
 * Language metadata for supported locales.
 *
 * To add a new language:
 * 1. Create a new folder: src/i18n/locales/{code}/translation.json
 * 2. Add an entry here with the language code, English name, and native name
 */
export const LANGUAGE_METADATA: Record<
  string,
  { name: string; nativeName: string }
> = {
  en: { name: "English", nativeName: "English" },
  de: { name: "German", nativeName: "Deutsch" },
  es: { name: "Spanish", nativeName: "Español" },
  fr: { name: "French", nativeName: "Français" },
  ja: { name: "Japanese", nativeName: "日本語" },
  vi: { name: "Vietnamese", nativeName: "Tiếng Việt" },
  zh: { name: "Chinese", nativeName: "中文" },
};

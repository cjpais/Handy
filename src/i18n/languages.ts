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
  es: { name: "Spanish", nativeName: "Español" },
  fr: { name: "French", nativeName: "Français" },
  vi: { name: "Vietnamese", nativeName: "Tiếng Việt" },
};

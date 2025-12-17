/**
 * Language metadata for supported locales.
 *
 * To add a new language:
 * 1. Create a new folder: src/i18n/locales/{code}/translation.json
 * 2. Add an entry here with the language code, English name, and native name
 * 3. Optionally add a priority (lower = higher in dropdown, no priority = alphabetical at end)
 */
export const LANGUAGE_METADATA: Record<
  string,
  { name: string; nativeName: string; priority?: number }
> = {
  en: { name: "English", nativeName: "English", priority: 1 },
  zh: { name: "Chinese", nativeName: "中文", priority: 2 },
  es: { name: "Spanish", nativeName: "Español", priority: 3 },
  fr: { name: "French", nativeName: "Français", priority: 4 },
  de: { name: "German", nativeName: "Deutsch", priority: 5 },
  ja: { name: "Japanese", nativeName: "日本語", priority: 6 },
  vi: { name: "Vietnamese", nativeName: "Tiếng Việt", priority: 7 },
  pl: { name: "Polish", nativeName: "Polski", priority: 8 },
  it: { name: "Italian", nativeName: "Italiano", priority: 9 },
};

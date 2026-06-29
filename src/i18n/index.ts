import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import { LANGUAGE_METADATA } from "./languages";
import {
  getLanguageDirection,
  updateDocumentDirection,
  updateDocumentLanguage,
} from "@/lib/utils/rtl";

// Auto-discover translation files using Vite's glob import
const localeModules = import.meta.glob<{ default: Record<string, unknown> }>(
  "./locales/*/translation.json",
  { eager: true },
);

// Build resources from discovered locale files
const resources: Record<string, { translation: Record<string, unknown> }> = {};
for (const [path, module] of Object.entries(localeModules)) {
  const langCode = path.match(/\.\/locales\/(.+)\/translation\.json/)?.[1];
  if (langCode) {
    resources[langCode] = { translation: module.default };
  }
}

// Build supported languages list from discovered locales + metadata
export const SUPPORTED_LANGUAGES = Object.keys(resources)
  .map((code) => {
    const meta = LANGUAGE_METADATA[code];
    if (!meta) {
      console.warn(`Missing metadata for locale "${code}" in languages.ts`);
      return { code, name: code, nativeName: code, priority: undefined };
    }
    return {
      code,
      name: meta.name,
      nativeName: meta.nativeName,
      priority: meta.priority,
    };
  })
  .sort((a, b) => {
    // Sort by priority first (lower = higher), then alphabetically
    if (a.priority !== undefined && b.priority !== undefined) {
      return a.priority - b.priority;
    }
    if (a.priority !== undefined) return -1;
    if (b.priority !== undefined) return 1;
    return a.name.localeCompare(b.name);
  });

export type SupportedLanguageCode = string;

// Initialize i18n with English as default
// Language will be synced from settings after init
i18n.use(initReactI18next).init({
  resources,
  lng: "en",
  fallbackLng: "en",
  interpolation: {
    escapeValue: false, // React already escapes values
  },
  react: {
    useSuspense: false, // Disable suspense for SSR compatibility
  },
});

// Sync language from app settings
export const syncLanguageFromSettings = async () => {
  try {
    // Keep the app in English even if older settings or system locale differ.
    if (i18n.language !== "en") {
      await i18n.changeLanguage("en");
    }
  } catch (e) {
    console.warn("Failed to sync language from settings:", e);
  }
};

// Run language sync on init
syncLanguageFromSettings();

// Listen for language changes to update HTML dir and lang attributes
i18n.on("languageChanged", (lng) => {
  const dir = getLanguageDirection(lng);
  updateDocumentDirection(dir);
  updateDocumentLanguage(lng);
});

// Re-export RTL utilities for convenience
export { getLanguageDirection, isRTLLanguage } from "@/lib/utils/rtl";

export default i18n;

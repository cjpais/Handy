import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import { locale } from "@tauri-apps/plugin-os";
import { LANGUAGE_METADATA } from "./languages";
import { commands } from "@/bindings";
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

const TRADITIONAL_CHINESE_REGIONS = new Set(["tw", "hk", "mo"]);
const SIMPLIFIED_CHINESE_REGIONS = new Set(["cn", "sg"]);

const getLocaleSubtags = (langCode: string) =>
  langCode.trim().toLowerCase().replace(/_/g, "-").split("-");

const findScriptAndRegion = (subtags: string[]) => {
  let script: string | undefined;
  let region: string | undefined;

  for (const subtag of subtags.slice(1)) {
    // A singleton begins a BCP-47 extension, so later values are not part of
    // the language/script/region identifier.
    if (subtag.length === 1) break;
    if (!script && /^[a-z]{4}$/.test(subtag)) script = subtag;
    if (!region && (/^[a-z]{2}$/.test(subtag) || /^\d{3}$/.test(subtag))) {
      region = subtag;
    }
  }

  return { script, region };
};

// Check if a language code is supported
export const getSupportedLanguage = (
  langCode: string | null | undefined,
): SupportedLanguageCode | null => {
  if (!langCode) return null;

  const subtags = getLocaleSubtags(langCode);
  const normalized = subtags.join("-");

  // Try an exact, case- and separator-insensitive match first.
  let supported = SUPPORTED_LANGUAGES.find(
    (lang) => getLocaleSubtags(lang.code).join("-") === normalized,
  );

  if (!supported) {
    const language = subtags[0];
    const { script, region } = findScriptAndRegion(subtags);
    let fallbackCode: string | undefined;

    if (
      language === "zh" &&
      (script === "hant" ||
        (script !== "hans" &&
          region !== undefined &&
          TRADITIONAL_CHINESE_REGIONS.has(region)))
    ) {
      // Traditional Chinese may be identified by script or only by region.
      fallbackCode = "zh-tw";
    } else if (language === "yue") {
      // We do not have a Cantonese translation. Use the matching Chinese
      // script, with Traditional as Cantonese's default writing system.
      fallbackCode =
        script === "hans" ||
        (script !== "hant" &&
          region !== undefined &&
          SIMPLIFIED_CHINESE_REGIONS.has(region))
          ? "zh"
          : "zh-tw";
    }

    if (fallbackCode) {
      supported = SUPPORTED_LANGUAGES.find(
        (lang) => lang.code.toLowerCase() === fallbackCode,
      );
    }
  }

  if (!supported) {
    // Fall back to the language subtag.
    supported = SUPPORTED_LANGUAGES.find(
      (lang) => lang.code.toLowerCase() === subtags[0],
    );
  }

  return supported ? supported.code : null;
};

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
    const result = await commands.getAppSettings();
    if (result.status === "ok" && result.data.app_language) {
      const supported = getSupportedLanguage(result.data.app_language);
      if (supported && supported !== i18n.language) {
        await i18n.changeLanguage(supported);
      }
    } else {
      // Fall back to system locale detection if no saved preference
      const systemLocale = await locale();
      const supported = getSupportedLanguage(systemLocale);
      if (supported && supported !== i18n.language) {
        await i18n.changeLanguage(supported);
      }
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

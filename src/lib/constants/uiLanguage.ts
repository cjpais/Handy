export const UI_LANGUAGE_STORAGE_KEY = "ui_language" as const;

export type UILanguage = "en" | "fr" | "es";

export const SUPPORTED_UI_LANGUAGES: UILanguage[] = ["en", "fr", "es"];

export const UI_LANGUAGE_CHANGED_EVENT = "ui-language-changed" as const;

export const normalizeUiLanguage = (
  value: string | null | undefined,
): UILanguage => {
  if (!value) {
    return "en";
  }

  const normalized = value.toLowerCase().slice(0, 2);
  if (normalized === "fr") return "fr";
  if (normalized === "es") return "es";
  return "en";
};

export const getStoredUiLanguage = (): UILanguage | null => {
  if (typeof window === "undefined") {
    return null;
  }

  const stored = window.localStorage.getItem(UI_LANGUAGE_STORAGE_KEY);
  return stored ? normalizeUiLanguage(stored) : null;
};

export const setStoredUiLanguage = (language: UILanguage) => {
  if (typeof window === "undefined") {
    return;
  }

  window.localStorage.setItem(UI_LANGUAGE_STORAGE_KEY, language);
  window.dispatchEvent(
    new CustomEvent<UILanguage>(UI_LANGUAGE_CHANGED_EVENT, { detail: language }),
  );
};

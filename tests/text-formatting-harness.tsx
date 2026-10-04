import React from "react";
import { createRoot } from "react-dom/client";
import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import translations from "../src/i18n/locales/en/translation.json";
import { TextFormatting } from "../src/components/settings/TextFormatting";
import { useSettingsStore } from "../src/stores/settingsStore";
import type {
  AppSettings,
  TextFormatting as Formatting,
} from "../src/bindings";
import "../src/App.css";

const formatting: Formatting = {
  enabled: true,
  spoken_punctuation: true,
  initial_capitalization: "lower",
  periods: "keep",
  replacements: [
    { phrase: "exclamation point", replacement: "!" },
    { phrase: "new line", replacement: "\n" },
  ],
};
await i18n
  .use(initReactI18next)
  .init({ lng: "en", resources: { en: { translation: translations } } });
useSettingsStore.setState({
  isLoading: false,
  settings: { text_formatting: formatting } as AppSettings,
  updateSetting: async (key, value) => {
    useSettingsStore.setState((state) => ({
      settings: { ...state.settings, [key]: value } as AppSettings,
    }));
  },
});
createRoot(document.getElementById("root")!).render(
  <div className="max-w-3xl mx-auto p-4">
    <TextFormatting />
  </div>,
);

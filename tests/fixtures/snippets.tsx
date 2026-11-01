// UI fixture only: actual component, in-memory writes, no audio or Tauri IPC.
import React from "react";
import { createRoot } from "react-dom/client";
import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import translations from "../../src/i18n/locales/en/translation.json";
import { SnippetsSettings } from "../../src/components/settings/Snippets";
import { Sidebar } from "../../src/components/Sidebar";
import { useSettingsStore } from "../../src/stores/settingsStore";
import "../../src/App.css";

const fixture = { store: useSettingsStore, failNextSave: false };
Object.assign(window, { snippetsFixture: fixture });
await i18n.use(initReactI18next).init({
  lng: "en",
  fallbackLng: "en",
  resources: { en: { translation: translations } },
  interpolation: { escapeValue: false },
});
useSettingsStore.setState({
  isLoading: false,
  settings: { snippets: [], snippets_enabled: false },
  updateSetting: async (key, value) => {
    if (fixture.failNextSave) {
      fixture.failNextSave = false;
      throw new Error("Simulated persistence failure");
    }
    useSettingsStore.setState((state) => ({
      settings: { ...state.settings, [key]: value },
    }));
  },
});
createRoot(document.getElementById("root")!).render(
  <div className="flex min-h-screen bg-background text-text p-4 gap-6">
    <Sidebar activeSection="snippets" onSectionChange={() => {}} />
    <SnippetsSettings />
  </div>,
);

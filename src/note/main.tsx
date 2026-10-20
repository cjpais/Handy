import React from "react";
import ReactDOM from "react-dom/client";
import { platform } from "@tauri-apps/plugin-os";
import { listen } from "@tauri-apps/api/event";
import { PracticeNote } from "./PracticeNote";
import {
  applyTheme,
  getStoredTheme,
  syncThemeFromSettings,
} from "@/lib/utils/theme";
import type { Theme } from "@/bindings";
import { syncLanguageFromSettings } from "@/i18n";
import "@/App.css";

// Onboarding's practice note: its own webview, so it sets the theme and
// language itself, like the overlay does.
document.documentElement.dataset.platform = platform();
applyTheme(getStoredTheme());
syncThemeFromSettings();
syncLanguageFromSettings();
listen<Theme>("theme-changed", (event) => applyTheme(event.payload));

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <PracticeNote />
  </React.StrictMode>,
);

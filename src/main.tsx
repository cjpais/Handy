import React from "react";
import ReactDOM from "react-dom/client";
import { platform } from "@tauri-apps/plugin-os";
import App from "./App";
import TitleBar from "./components/TitleBar";
import { installCompatShims } from "./lib/compat";
import {
  applyTheme,
  getStoredTheme,
  syncThemeFromSettings,
} from "./lib/utils/theme";

installCompatShims();

// Set platform before render so CSS can scope per-platform (e.g. scrollbar styles)
const currentPlatform = platform();
document.documentElement.dataset.platform = currentPlatform;

// Apply the last-known theme synchronously before render to avoid a flash of
// the wrong palette, then reconcile with the persisted setting once it loads.
applyTheme(getStoredTheme());
syncThemeFromSettings();

// Initialize i18n
import "./i18n";

// Initialize model store (loads models and sets up event listeners)
import { useModelStore } from "./stores/modelStore";
useModelStore.getState().initialize();

// Windows: the main window is undecorated (see lib.rs), so draw a title bar in
// its own root above #root. Kept out of App so the window controls work while
// App is still loading or has crashed.
if (currentPlatform === "windows") {
  const titleBarRoot = document.createElement("div");
  document.body.prepend(titleBarRoot);
  ReactDOM.createRoot(titleBarRoot).render(
    <React.StrictMode>
      <TitleBar />
    </React.StrictMode>,
  );
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);

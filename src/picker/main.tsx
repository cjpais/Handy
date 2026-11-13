import React from "react";
import ReactDOM from "react-dom/client";
import WindowPicker from "./WindowPicker";
import "@/i18n";

// The picker covers other apps, so a stray right-click must not open the
// webview's default menu. Kept in dev for Inspect.
if (import.meta.env.PROD) {
  window.addEventListener("contextmenu", (e) => e.preventDefault());
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <WindowPicker />
  </React.StrictMode>,
);

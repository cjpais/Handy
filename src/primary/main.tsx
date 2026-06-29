import React from "react";
import ReactDOM from "react-dom/client";
import { platform } from "@tauri-apps/plugin-os";
import PrimaryApp from "./PrimaryApp";

// Load the shared stylesheet (Tailwind + theme variables + fonts)
import "../App.css";

document.documentElement.dataset.platform = platform();

import "../i18n";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <PrimaryApp />
  </React.StrictMode>,
);

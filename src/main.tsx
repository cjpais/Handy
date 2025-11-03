import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./i18n"; // 🔹 initialise i18n ici

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);

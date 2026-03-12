import React from "react";
import ReactDOM from "react-dom/client";
import "@/i18n";
import "./storybook.css";
import { StorybookApp } from "./storybook";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <StorybookApp />
  </React.StrictMode>,
);

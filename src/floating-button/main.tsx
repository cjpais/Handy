import React from "react";
import ReactDOM from "react-dom/client";
import FloatingButton from "./FloatingButton";
import "@/i18n";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <FloatingButton />
  </React.StrictMode>,
);

import React from "react";
import ReactDOM from "react-dom/client";
import AgentReviewOverlay from "./AgentReviewOverlay";
import RecordingOverlay from "./RecordingOverlay";
import "@/i18n";

const overlay = new URLSearchParams(window.location.search).get("overlay");
const OverlayComponent =
  overlay === "agent_review" ? AgentReviewOverlay : RecordingOverlay;

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <OverlayComponent />
  </React.StrictMode>,
);

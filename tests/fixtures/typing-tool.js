import React from "react";
import "../../src/App.css";
import { createRoot } from "react-dom/client";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import { mockIPC } from "@tauri-apps/api/mocks";
import { TypingToolSetting } from "../../src/components/settings/TypingTool";
import { useSettingsStore } from "../../src/stores/settingsStore";
import translation from "../../src/i18n/locales/en/translation.json";

// Keep the real component, dropdown, settings store and translations. Only the
// desktop boundary is mocked so the fixture can run in an ordinary browser.
window.__TAURI_OS_PLUGIN_INTERNALS__ = { os_type: "linux" };
let resolveTools;
const availableTools = new Promise((resolve) => {
  resolveTools = resolve;
});
const writes = [];
mockIPC((command, payload) => {
  if (command === "get_available_typing_tools") return availableTools;
  if (command === "change_typing_tool_setting") {
    writes.push(payload.tool);
    return null;
  }
  throw new Error(`Unexpected command: ${command}`);
});
useSettingsStore.setState({
  settings: {
    paste_method: "direct",
    typing_tool: new URLSearchParams(location.search).get("saved") ?? "auto",
  },
  isLoading: false,
});
window.typingToolFixture = {
  resolveTools: (tools) => resolveTools(tools),
  writes,
  selected: () => useSettingsStore.getState().getSetting("typing_tool"),
};
// Optional deterministic data for manually inspecting this browser fixture.
const previewTools = new URLSearchParams(location.search).get("tools");
if (previewTools) resolveTools(previewTools.split(","));
await i18next.use(initReactI18next).init({
  lng: "en",
  resources: { en: { translation } },
});
createRoot(document.getElementById("root")).render(
  React.createElement(TypingToolSetting, { descriptionMode: "inline" }),
);

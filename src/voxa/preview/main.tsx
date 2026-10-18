/**
 * Dev-only preview page (`/voxa-preview`). It renders Voxa's UI in a plain
 * browser so it can be reviewed and screenshotted without a Mac:
 *
 *   ?view=overlay[&lang=ru|en][&only=<stage id>]   the capsule in every state
 *   ?view=stats[&theme=light|dark][&data=empty|error]   the statistics screen
 *   ?view=app[&theme=light|dark][&update=available|downloading|error|up_to_date]
 *                                                  the whole app on mocked data
 *
 * The Tauri backend is replaced by fixtures (mockBackend.ts).
 */
import React from "react";
import ReactDOM from "react-dom/client";
import type { UpdateFixtureKind } from "./fixtures";

const params = new URLSearchParams(window.location.search);
const view = params.get("view") ?? "overlay";
const theme = (params.get("theme") as "light" | "dark" | null) ?? "dark";
const lang = (params.get("lang") as "ru" | "en" | null) ?? "ru";
const data = params.get("data");

// What the Tauri plugins inject into a real window.
Object.assign(window, {
  __TAURI_OS_PLUGIN_INTERNALS__: {
    platform: "macos",
    family: "unix",
    version: "15.0.0",
    type: "macos",
    arch: "aarch64",
    eol: "\n",
    exe_extension: "",
  },
});

async function boot() {
  const root = ReactDOM.createRoot(
    document.getElementById("root") as HTMLElement,
  );
  document.documentElement.dataset.platform = "macos";
  document.documentElement.lang = lang;

  if (view === "overlay") {
    document.body.classList.add("pv");
    const { OverlayGallery } = await import("./OverlayGallery");
    root.render(<OverlayGallery lang={lang} only={params.get("only")} />);
    return;
  }

  const { installMockBackend } = await import("./mockBackend");
  installMockBackend({
    theme,
    language: lang,
    stats: data === "empty" || data === "error" ? data : undefined,
    update: (params.get("update") as UpdateFixtureKind | null) ?? undefined,
  });

  await import("@/App.css");
  const i18nModule = await import("@/i18n");
  await i18nModule.default.changeLanguage(lang);
  const { applyTheme } = await import("@/lib/utils/theme");
  applyTheme(theme);

  if (view === "stats") {
    const { StatsView } = await import("../stats/StatsView");
    const { statsFixture } = await import("./fixtures");
    root.render(
      <div style={{ padding: 16, width: 560, boxSizing: "border-box" }}>
        <StatsView
          stats={statsFixture()}
          locale={lang}
          pendingTerm={null}
          onAddWord={() => {}}
          onClear={() => {}}
          onRefresh={() => {}}
        />
      </div>,
    );
    return;
  }

  // view === "app"
  const { useModelStore } = await import("@/stores/modelStore");
  await useModelStore.getState().initialize();
  const { default: App } = await import("@/App");
  root.render(<App />);
}

void boot();

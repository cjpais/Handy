import React, { useState } from "react";
import { createRoot } from "react-dom/client";
import i18next from "i18next";
import { I18nextProvider, useTranslation } from "react-i18next";
import { SettingContainer } from "../../src/components/ui/SettingContainer";
import { Tooltip } from "../../src/components/ui/Tooltip";
import { Dialog } from "../../src/components/ui/Dialog";
import en from "../../src/i18n/locales/en/translation.json";
import ko from "../../src/i18n/locales/ko/translation.json";
import "../../src/App.css";

const params = new URLSearchParams(location.search);
const i18n = i18next.createInstance();
await i18n.init({
  lng: params.get("lng") || "en",
  fallbackLng: "en",
  resources: { en: { translation: en }, ko: { translation: ko } },
  interpolation: { escapeValue: false },
});

function HelpSetting() {
  const { t } = useTranslation();
  return (
    <SettingContainer
      title={t("settings.sound.volume.title")}
      description={t("settings.sound.volume.description")}
      grouped={params.has("grouped")}
      layout={params.get("layout") === "stacked" ? "stacked" : "horizontal"}
    >
      <button type="button">Setting action</button>
    </SettingContainer>
  );
}

function Fixture() {
  const [outerEscapes, setOuterEscapes] = useState(0);
  const [dialogOpen, setDialogOpen] = useState(false);
  return (
    <main
      style={{
        padding: params.has("edge") ? 0 : 110,
        maxWidth: 900,
        display: "grid",
        gap: 24,
      }}
      onKeyDown={(event) => {
        if (event.key === "Escape") setOuterEscapes((value) => value + 1);
      }}
    >
      <section data-testid="primary-help">
        <HelpSetting />
      </section>
      <Tooltip title="Details" text="Sibling description">
        <button type="button" data-testid="sibling-trigger">
          Hover help
        </button>
      </Tooltip>
      <button
        type="button"
        data-testid="open-dialog"
        onClick={() => setDialogOpen(true)}
      >
        Open dialog
      </button>
      <Tooltip text="Input hint">
        <input
          aria-label="IME input"
          defaultValue="한글"
          onKeyDown={(event) => {
            if (params.has("child-handles-escape") && event.key === "Escape")
              event.preventDefault();
          }}
        />
      </Tooltip>
      <output data-testid="outer-escapes">{outerEscapes}</output>
      <Dialog
        open={dialogOpen}
        onOpenChange={setDialogOpen}
        title="Tooltip test"
        closeLabel="Close dialog"
        contentFades={false}
      >
        <section data-testid="dialog-help">
          <HelpSetting />
        </section>
      </Dialog>
    </main>
  );
}

createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <I18nextProvider i18n={i18n}>
      <Fixture />
    </I18nextProvider>
  </React.StrictMode>,
);

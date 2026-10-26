import React, { useState } from "react";
import { createRoot } from "react-dom/client";
import i18next from "i18next";
import { I18nextProvider, useTranslation } from "react-i18next";
import { ShortcutInput } from "../../src/components/settings/ShortcutInput";
import { Slider } from "../../src/components/ui/Slider";
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

function ResetNamesFixture() {
  const { t } = useTranslation();
  const [value, setValue] = useState(0.8);
  const [resets, setResets] = useState(0);
  return (
    <main style={{ display: "grid", gap: 24, padding: 24, maxWidth: 800 }}>
      <button onClick={() => void i18n.changeLanguage("ko")}>
        Switch to Korean
      </button>
      {["transcribe", "cancel", "transcribe_with_post_process", "custom"].map(
        (id) => (
          <section key={id} data-testid={id}>
            <ShortcutInput shortcutId={id} descriptionMode="inline" />
          </section>
        ),
      )}
      <section data-testid="volume">
        <Slider
          value={value}
          onChange={setValue}
          min={0}
          max={1}
          label={t("settings.sound.volume.title")}
          description={t("settings.sound.volume.description")}
          descriptionMode="inline"
          isResetting={params.has("busy")}
          onReset={() => {
            setValue(0.5);
            setResets((count) => count + 1);
          }}
        />
      </section>
      <output data-testid="volume-resets">{resets}</output>
    </main>
  );
}

createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <I18nextProvider i18n={i18n}>
      <ResetNamesFixture />
    </I18nextProvider>
  </React.StrictMode>,
);

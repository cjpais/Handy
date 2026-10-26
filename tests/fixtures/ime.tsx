import React, { useState } from "react";
import { createRoot } from "react-dom/client";
import i18next from "i18next";
import { I18nextProvider } from "react-i18next";
import { CustomWords } from "../../src/components/settings/CustomWords";
import { LanguageSelector } from "../../src/components/settings/LanguageSelector";
import { PreferredLanguages } from "../../src/components/settings/PreferredLanguages";
import {
  ModelTable,
  NO_FILTERS,
} from "../../src/components/shared/modelTable/ModelTable";
import {
  useModelTable,
  type TableModel,
} from "../../src/components/shared/modelTable/context";
import type { Grading } from "../../src/lib/models/grading";
import en from "../../src/i18n/locales/en/translation.json";
import ko from "../../src/i18n/locales/ko/translation.json";
import "../../src/App.css";

const params = new URLSearchParams(location.search);
const i18n = i18next.createInstance();
await i18n.init({
  lng: params.get("lng") || "ko",
  fallbackLng: "en",
  resources: { en: { translation: en }, ko: { translation: ko } },
  interpolation: { escapeValue: false },
});

const models: TableModel[] = ["Alpha", "Beta"].map((name) => ({
  id: name.toLowerCase(),
  name,
  description: name,
  languages: ["ko"],
  streaming: false,
  translate: false,
  sizeMb: 10,
  quant: null,
  kind: "custom",
}));
const grading: Grading = {
  langs: ["ko"],
  tier: "cpu",
  roleOf: () => null,
  grade: () => ({ letter: "none", label: "?" }),
  overall: () => ({ letter: "none", label: "?" }),
  wait: () => undefined,
  missing: () => [],
};

function ModelRow({
  entry,
  onSelect,
}: {
  entry: TableModel;
  onSelect: (id: string) => void;
}) {
  const { highlighted } = useModelTable();
  return (
    <button
      type="button"
      role="radio"
      data-row={entry.id}
      data-highlighted={highlighted === entry.id}
      aria-checked={entry.id === "alpha"}
      onClick={() => onSelect(entry.id)}
    >
      {entry.name}
    </button>
  );
}
function ModelFixture() {
  const [filters, setFilters] = useState(NO_FILTERS);
  const [selections, setSelections] = useState<string[]>([]);
  const [escapes, setEscapes] = useState(0);
  return (
    <>
      <div style={{ height: 360, display: "flex" }}>
        <ModelTable
          grading={grading}
          filters={filters}
          setFilters={setFilters}
          sections={[{ key: "models", label: "Models", rows: models }]}
          renderRow={(entry) => (
            <ModelRow
              key={entry.id}
              entry={entry}
              onSelect={(id) => setSelections((value) => [...value, id])}
            />
          )}
          onEscape={() => setEscapes((value) => value + 1)}
        />
      </div>
      <output data-testid="selected-models">
        {JSON.stringify(selections)}
      </output>
      <output data-testid="model-escapes">{escapes}</output>
    </>
  );
}
function Fixture() {
  const view = params.get("view");
  return (
    <main style={{ padding: 24, maxWidth: 800 }} data-testid="ready">
      {view === "words" && <CustomWords descriptionMode="inline" />}
      {view === "language" && (
        <LanguageSelector
          descriptionMode="inline"
          supportedLanguages={["en", "ko"]}
        />
      )}
      {view === "preferred" && <PreferredLanguages descriptionMode="inline" />}
      {view === "model" && <ModelFixture />}
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

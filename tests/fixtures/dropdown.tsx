import React, { useState } from "react";
import { createRoot } from "react-dom/client";
import i18next from "i18next";
import { I18nextProvider } from "react-i18next";
import { Dropdown } from "../../src/components/ui/Dropdown";
import { Dialog } from "../../src/components/ui/Dialog";
import "../../src/App.css";

const i18n = i18next.createInstance();
await i18n.init({
  lng: "en",
  fallbackLng: "en",
  resources: {
    en: { translation: { common: { noOptionsFound: "No options found" } } },
  },
  interpolation: { escapeValue: false },
});

const options = [
  { value: "one", label: "First choice" },
  { value: "two", label: "Second choice" },
];
const aboveOptions = [
  { value: "above-one", label: "Above first" },
  { value: "above-two", label: "Above second" },
];

function DropdownFixture() {
  const [selection, setSelection] = useState("one");
  const [aboveSelection, setAboveSelection] = useState("above-one");
  const [opens, setOpens] = useState(0);
  const [selections, setSelections] = useState(0);
  const [outerEscapes, setOuterEscapes] = useState(0);
  const [dialogOpen, setDialogOpen] = useState(false);

  return (
    <main
      style={{
        display: "grid",
        gap: 48,
        padding: "160px 24px 40px",
        maxWidth: 500,
      }}
      onKeyDown={(event) => {
        if (event.key === "Escape") setOuterEscapes((count) => count + 1);
      }}
    >
      <section data-testid="default-menu">
        <Dropdown
          options={options}
          selectedValue={selection}
          onSelect={(value) => {
            setSelection(value);
            setSelections((count) => count + 1);
          }}
          onOpen={() => setOpens((count) => count + 1)}
        />
      </section>
      <section data-testid="above-menu">
        <Dropdown
          options={aboveOptions}
          selectedValue={aboveSelection}
          onSelect={setAboveSelection}
          placement="above"
          tone="neutral"
        />
      </section>
      <section data-testid="empty-menu">
        <Dropdown
          options={[]}
          selectedValue={null}
          onSelect={() => undefined}
          placeholder="Empty menu"
        />
      </section>
      <div>
        <output data-testid="selection">{selection}</output>
        <output data-testid="opens">{opens}</output>
        <output data-testid="selections">{selections}</output>
        <output data-testid="outer-escapes">{outerEscapes}</output>
      </div>
      <button data-testid="open-dialog" onClick={() => setDialogOpen(true)}>
        Open dialog
      </button>
      <Dialog
        open={dialogOpen}
        onOpenChange={setDialogOpen}
        title="Dropdown keyboard test"
        closeLabel="Close dialog"
        contentFades={false}
      >
        <div style={{ minHeight: 180 }} data-testid="dialog-menu">
          <Dropdown
            options={options}
            selectedValue={selection}
            onSelect={setSelection}
          />
        </div>
      </Dialog>
    </main>
  );
}

createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <I18nextProvider i18n={i18n}>
      <DropdownFixture />
    </I18nextProvider>
  </React.StrictMode>,
);

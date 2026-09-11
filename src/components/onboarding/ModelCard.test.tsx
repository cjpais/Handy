// Standalone assert check. Run with:
//   bun src/components/onboarding/ModelCard.test.tsx
import assert from "node:assert/strict";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { createInstance } from "i18next";
import { I18nextProvider } from "react-i18next";
import type { ModelInfo } from "@/bindings";
import catalog from "../../../src-tauri/src/catalog/catalog.json";
import en from "../../i18n/locales/en/translation.json";
import { resolveWer } from "../../lib/utils/modelBenchmarks";
import ModelCard from "./ModelCard";

const modelFor = (slug: string): ModelInfo => {
  const entry = catalog.models.find((model) => model.slug === slug)!;
  return {
    id: entry.id,
    name: entry.name,
    description: entry.description,
    filename: `${slug}-Q8_0.gguf`,
    source: { HuggingFace: { repo_id: entry.id, revision: "test" } },
    size_mb: 100,
    is_downloaded: true,
    is_downloading: false,
    partial_size: 0,
    is_directory: false,
    engine_type: "TranscribeCpp",
    accuracy_score: 0.8,
    speed_score: 0.5,
    benchmarks: entry.benchmarks,
    supports_translation: false,
    is_recommended: false,
    supported_languages: entry.languages,
    supports_language_selection: true,
    is_custom: false,
    supports_streaming: false,
    supports_language_detection: true,
  };
};

const i18n = createInstance();
await i18n.init({
  lng: "en",
  resources: { en: { translation: en } },
  interpolation: { escapeValue: false },
});
const render = (model: ModelInfo, language: string) =>
  renderToStaticMarkup(
    <I18nextProvider i18n={i18n}>
      <ModelCard model={model} languageFilter={language} onSelect={() => {}} />
    </I18nextProvider>,
  );

// A vendor CER result must retain its character label in the card.
const qwen = modelFor("Qwen3-ASR-1.7B");
const korean = render(qwen, "ko");
assert.match(korean, /CER/);
assert.ok(korean.includes(en.onboarding.modelCard.characterAccuracy));

// Prefer Handy's language-specific measurement, then the vendor's.
assert.equal(resolveWer(qwen.benchmarks, "en", "Q8_0").wer?.source, "measured");
assert.equal(resolveWer(qwen.benchmarks, "ru", "Q8_0").wer?.source, "reported");

// Legacy accuracy fallback and speed both disclose the borrowed GGUF source.
const turbo = modelFor("whisper-large-v3-turbo");
const native = render(turbo, "ru");
turbo.source = { Url: { url: "https://example.com/model.bin", sha256: null } };
const legacy = render(turbo, "ru");
for (const pattern of [
  /title="([^"]*No benchmark for Russian[^"]*)"/,
  /title="([^"]*Transcribes[^"]*)"/,
]) {
  const nativeTitle = native.match(pattern)?.[1];
  const legacyTitle = legacy.match(pattern)?.[1];
  assert.ok(nativeTitle);
  assert.equal(
    legacyTitle,
    `${nativeTitle} ${en.onboarding.modelCard.legacyTooltip}`,
  );
}

console.log("ModelCard: all assertions passed");

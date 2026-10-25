import catalog from "../src-tauri/src/catalog/catalog.json";
import { LANGUAGES, supportsLanguage } from "../src/lib/constants/languages.ts";

interface CatalogModel {
  name: string;
  role: string;
  languages?: string[];
}

const failures: string[] = [];

// These code variants must share one saved language so switching model families
// keeps it. Mirrors `canonical_language_code` in src-tauri/src/managers/model.rs.
for (const [a, b] of [
  ["no", "nb"],
  ["tl", "fil"],
  ["iw", "he"],
  ["jw", "jv"],
]) {
  if (!supportsLanguage([a], b) || !supportsLanguage([b], a)) {
    failures.push(`Language code \`${a}\` must remain equivalent to \`${b}\``);
  }
}

// Only transcription models reach the language picker; the catalog also lists
// the language-ID, VAD and diarization models transcribe.cpp publishes.
const transcriptionModels = (catalog.models as CatalogModel[]).filter(
  (model) => model.role === "asr",
);

for (const model of transcriptionModels) {
  for (const modelLanguage of model.languages ?? []) {
    const matching = LANGUAGES.filter((language) =>
      supportsLanguage([modelLanguage], language.code),
    );

    if (matching.length !== 1) {
      const matchSummary =
        matching.length === 0
          ? "no frontend language"
          : `ambiguous languages: ${matching
              .map((language) => language.code)
              .join(", ")}`;
      failures.push(`${model.name}: ${modelLanguage} (${matchSummary})`);
    }
  }
}

if (failures.length > 0) {
  console.error("Model language coverage check failed:\n");
  for (const failure of failures) console.error(`  - ${failure}`);
  process.exit(1);
}

console.log(
  `Model language coverage: all catalog codes map to exactly one of ${LANGUAGES.length} frontend languages`,
);

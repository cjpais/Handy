import catalog from "../src-tauri/src/catalog/catalog.json";
import {
  MODEL_CAPABILITY_LANGUAGES,
  supportsLanguageCode,
} from "../src/lib/constants/languages.ts";

interface CatalogModel {
  name: string;
  languages?: string[];
}

const failures: string[] = [];

// These model-code variants intentionally share one persisted UI intent. Keep
// this assertion beside catalog coverage so adding a separate picker entry
// cannot silently break language continuity when users switch model families.
if (
  !supportsLanguageCode(["nb"], "no") ||
  !supportsLanguageCode(["no"], "nb")
) {
  failures.push(
    "Norwegian intent `no` must remain equivalent to model code `nb`",
  );
}

for (const model of catalog.models as CatalogModel[]) {
  for (const modelLanguage of model.languages ?? []) {
    const matchingIntents = MODEL_CAPABILITY_LANGUAGES.filter((language) =>
      supportsLanguageCode([modelLanguage], language.value),
    );

    if (matchingIntents.length !== 1) {
      const matchSummary =
        matchingIntents.length === 0
          ? "no frontend language intent"
          : `ambiguous intents: ${matchingIntents
              .map((language) => language.value)
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
  `Model language coverage: all catalog codes map to exactly one of ${MODEL_CAPABILITY_LANGUAGES.length} frontend intents`,
);

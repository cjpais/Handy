import type { ModelInfo } from "@/bindings";

// Retired (ONNX) model ids mapped to the catalog repo that replaces each one.
// Whisper .bin models are legacy too, but still run, so they are not listed.
export const RETIRED_MODEL_REPLACEMENTS: Record<string, string> = {
  "parakeet-tdt-0.6b-v2": "handy-computer/parakeet-tdt-0.6b-v2-gguf",
  "parakeet-tdt-0.6b-v3": "handy-computer/parakeet-tdt-0.6b-v3-gguf",
  "moonshine-base": "handy-computer/moonshine-base-gguf",
  "moonshine-tiny-streaming-en": "handy-computer/moonshine-streaming-tiny-gguf",
  "moonshine-small-streaming-en":
    "handy-computer/moonshine-streaming-small-gguf",
  "moonshine-medium-streaming-en":
    "handy-computer/moonshine-streaming-medium-gguf",
  "sense-voice-int8": "handy-computer/SenseVoiceSmall-gguf",
  "gigaam-v3-e2e-ctc": "handy-computer/gigaam-v3-e2e-ctc-gguf",
  "canary-180m-flash": "handy-computer/canary-180m-flash-gguf",
  "canary-1b-v2": "handy-computer/canary-1b-v2-gguf",
  "cohere-int8": "handy-computer/cohere-transcribe-03-2026-gguf",
};

export interface ModelMigrationPair {
  retired: ModelInfo;
  replacement: ModelInfo;
}

const repoIdOf = (model: ModelInfo): string | null =>
  typeof model.source === "object" && "HuggingFace" in model.source
    ? model.source.HuggingFace.repo_id
    : null;

// Catalog model ids carry the default filename ("{repo}/{file}"), so the
// replacement is matched on repo id rather than model id.
export const findReplacement = (
  models: ModelInfo[],
  retiredId: string,
): ModelInfo | undefined => {
  const repoId = RETIRED_MODEL_REPLACEMENTS[retiredId];
  return models.find((m) => repoIdOf(m) === repoId);
};

export const isRetiredModel = (modelId: string): boolean =>
  modelId in RETIRED_MODEL_REPLACEMENTS;

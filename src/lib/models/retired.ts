import { commands, type ModelInfo, type RetiredModel } from "@/bindings";

// Retired (ONNX) models are defined in the backend (`RETIRED_ONNX_MODELS`);
// `commands.getRetiredModels()` reports the ones still on disk, each with the
// catalog repo that replaces it.

export interface ModelMigrationPair {
  retired: RetiredModel;
  replacement: ModelInfo;
}

export const repoIdOf = (model: ModelInfo): string | null =>
  typeof model.source === "object" && "HuggingFace" in model.source
    ? model.source.HuggingFace.repo_id
    : null;

// Catalog model ids carry the default filename ("{repo}/{file}"), so the
// replacement is matched on repo id rather than model id.
export const findReplacement = (
  models: ModelInfo[],
  retired: RetiredModel,
): ModelInfo | undefined =>
  models.find((m) => repoIdOf(m) === retired.replacement_repo);

// Best effort: a leftover retired model only wastes disk space, and the Models
// page notice offers to clean it up later.
export const deleteRetiredModel = async (modelId: string): Promise<boolean> => {
  const result = await commands.deleteRetiredModel(modelId);
  if (result.status === "error") {
    console.warn(`Failed to delete retired model ${modelId}:`, result.error);
  }
  return result.status === "ok";
};

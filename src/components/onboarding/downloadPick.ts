import { useModelStore } from "@/stores/modelStore";
import type { useOnboardingModels } from "./useOnboardingModels";

type Models = ReturnType<typeof useOnboardingModels>;

/**
 * Start on a newly picked model (catalog id): the download of the pick
 * before it (`downloadId`) stops if it's still going, and this one downloads
 * unless it's on disk. False when the app doesn't offer this model.
 */
export const downloadPick = (
  models: Models,
  id: string,
  downloadId: string | null,
): boolean => {
  const model = models.offered.find((m) => m.id === id);
  const info = model && models.infoFor(model);
  if (!model || !info) return false;
  const previous = models.offered.find((m) => m.id === downloadId);
  const previousInfo = previous && models.infoFor(previous);
  if (
    previousInfo &&
    previousInfo.id !== info.id &&
    models.downloadState(previous).state === "downloading"
  )
    useModelStore.getState().cancelDownload(previousInfo.id);
  // A new pick, or the same one again after its download stopped.
  if (
    !info.is_downloaded &&
    (downloadId !== id || models.downloadState(model).state === "failed")
  )
    models.startDownload(model);
  return true;
};

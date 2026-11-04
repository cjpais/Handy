import { useModelStore } from "@/stores/modelStore";
import { useModelCatalog } from "@/hooks/useModelCatalog";
import type { CatalogModel } from "@/lib/models/catalog";
import {
  downloadStateOf,
  type DownloadState,
} from "@/lib/models/downloadState";
import { useOnboardingStore } from "./onboardingStore";

export type { DownloadState };

/** The shared model catalog, plus onboarding's downloads (which can fail). */
export const useOnboardingModels = (languages: string[]) => {
  // Grades need a language; Welcome never continues without one.
  const models = useModelCatalog(languages.length ? languages : ["en"]);
  const sources = {
    models: useModelStore((s) => s.models),
    downloadingModels: useModelStore((s) => s.downloadingModels),
    verifyingModels: useModelStore((s) => s.verifyingModels),
    downloadProgress: useModelStore((s) => s.downloadProgress),
  };
  const download = useModelStore((s) => s.downloadModel);
  const failedId = useOnboardingStore((s) =>
    s.downloadFailed ? s.downloadId : null,
  );

  const downloadState = (model: CatalogModel | undefined): DownloadState => {
    const info = model && models.infoFor(model);
    if (!info) return { state: "none" };
    const s = downloadStateOf(sources, info.id);
    return s.state === "none" && model.id === failedId
      ? { state: "failed" }
      : s;
  };

  /** Download a model; if it stops while it's still the pick, say so. */
  const startDownload = (model: CatalogModel) => {
    const info = models.infoFor(model);
    if (!info) return;
    useOnboardingStore.getState().set({ downloadFailed: false });
    download(info.id).then((ok) => {
      const onboarding = useOnboardingStore.getState();
      const done = useModelStore
        .getState()
        .models.some((m) => m.id === info.id && m.is_downloaded);
      if (!ok && !done && onboarding.downloadId === model.id)
        onboarding.set({ downloadFailed: true });
    });
  };

  return { ...models, downloadState, startDownload };
};

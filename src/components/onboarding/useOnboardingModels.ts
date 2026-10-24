import { useEffect, useMemo, useState } from "react";
import { commands, type ModelInfo } from "@/bindings";
import { useModelStore } from "@/stores/modelStore";
import { useSettings } from "@/hooks/useSettings";
import { useOnboardingStore } from "./onboardingStore";
import {
  CURATED_ASR,
  groupModels,
  picksFor,
  type CatalogModel,
  type MachineTier,
} from "./data/models";
import { repoIdOf } from "./retiredModels";

/**
 * Which benchmark tier this computer is closest to: "accelerated" when the
 * engine sees a GPU and transcription isn't forced onto the CPU.
 */
export const useMachineTier = (): MachineTier => {
  const { getSetting } = useSettings();
  const accelerator = getSetting("transcribe_accelerator");
  const [hasGpu, setHasGpu] = useState(true);
  useEffect(() => {
    commands
      .getAvailableAccelerators()
      .then((a) => setHasGpu(a.gpu_devices.length > 0))
      .catch(() => setHasGpu(false));
  }, []);
  return hasGpu && accelerator !== "cpu" ? "accelerated" : "cpu";
};

export type DownloadState =
  | { state: "none" }
  | {
      state: "downloading";
      progress: number;
      etaSeconds: number | null;
      /** Bytes so far, of `total` (0 until the size is known). */
      downloaded: number;
      total: number;
      /** null until the speed is known. */
      bytesPerSec: number | null;
    }
  | { state: "checking" }
  | { state: "ready" }
  /** It stopped before it finished: failed, or cut off by a quit. */
  | { state: "failed" };

/**
 * The catalog's curated models joined to the app's own list (download state,
 * the id the backend knows them by). Only models the app can download are
 * offered.
 */
export const useOnboardingModels = (languages: string[]) => {
  const tier = useMachineTier();
  const infos = useModelStore((s) => s.models);
  const downloading = useModelStore((s) => s.downloadingModels);
  const verifying = useModelStore((s) => s.verifyingModels);
  const progress = useModelStore((s) => s.downloadProgress);
  const download = useModelStore((s) => s.downloadModel);
  const failedId = useOnboardingStore((s) =>
    s.downloadFailed ? s.downloadId : null,
  );

  const infoByRepo = useMemo(() => {
    const map = new Map<string, ModelInfo>();
    for (const info of infos) {
      const repo = repoIdOf(info);
      if (repo && !info.is_custom && !map.has(repo)) map.set(repo, info);
    }
    return map;
  }, [infos]);

  const infoFor = (model: CatalogModel) => infoByRepo.get(model.repository);

  const offered = useMemo(
    () => CURATED_ASR.filter((m) => infoByRepo.has(m.repository)),
    [infoByRepo],
  );

  const picks = useMemo(
    () => picksFor(languages, (m) => infoByRepo.has(m.repository)),
    [languages, infoByRepo],
  );

  const groups = useMemo(
    () => groupModels(languages, tier, offered, picks),
    [languages, tier, offered, picks],
  );

  /** A catalog model for a ModelInfo id (the backend's), if it's offered. */
  const catalogFor = (infoId: string): CatalogModel | undefined =>
    offered.find((m) => infoByRepo.get(m.repository)?.id === infoId);

  const downloadState = (model: CatalogModel | undefined): DownloadState => {
    const info = model && infoFor(model);
    if (!info) return { state: "none" };
    if (verifying[info.id]) return { state: "checking" };
    if (downloading[info.id]) {
      const p = progress[info.id];
      const rate = p?.bytes_per_sec;
      return {
        state: "downloading",
        progress: p ? p.percentage / 100 : 0,
        etaSeconds:
          p && p.total > 0 && rate && rate > 0
            ? (p.total - p.downloaded) / rate
            : null,
        downloaded: p?.downloaded ?? 0,
        total: p?.total ?? 0,
        bytesPerSec: rate && rate > 0 ? rate : null,
      };
    }
    if (info.is_downloaded) return { state: "ready" };
    if (model.id === failedId) return { state: "failed" };
    return { state: "none" };
  };

  /** Download a model; if it stops while it's still the pick, say so. */
  const startDownload = (model: CatalogModel) => {
    const info = infoFor(model);
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

  return {
    tier,
    offered,
    picks,
    groups,
    infoFor,
    catalogFor,
    downloadState,
    startDownload,
  };
};

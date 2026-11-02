import type { ModelInfo } from "@/bindings";
import type { DownloadProgress } from "@/stores/modelStore";

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

/** The parts of the model store a download's state is read from. */
interface DownloadSources {
  models: ModelInfo[];
  downloadingModels: Record<string, true>;
  verifyingModels: Record<string, true>;
  downloadProgress: Record<string, DownloadProgress>;
}

/** Where a model's download is, by the app's model id. Never "failed": the store doesn't track that. */
export const downloadStateOf = (
  s: DownloadSources,
  id: string,
): DownloadState => {
  if (s.verifyingModels[id]) return { state: "checking" };
  if (s.downloadingModels[id]) {
    const p = s.downloadProgress[id];
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
  return s.models.find((m) => m.id === id)?.is_downloaded
    ? { state: "ready" }
    : { state: "none" };
};

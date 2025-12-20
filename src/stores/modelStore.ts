import { create } from "zustand";
import { subscribeWithSelector } from "zustand/middleware";
import { listen } from "@tauri-apps/api/event";
import { commands, type ModelInfo } from "@/bindings";

interface DownloadProgress {
  model_id: string;
  downloaded: number;
  total: number;
  percentage: number;
}

interface DownloadStats {
  startTime: number;
  lastUpdate: number;
  totalDownloaded: number;
  speed: number; // MB/s
}

interface ModelsStore {
  models: ModelInfo[];
  currentModel: string;
  downloadingModels: Set<string>;
  extractingModels: Set<string>;
  downloadProgress: Map<string, DownloadProgress>;
  downloadStats: Map<string, DownloadStats>;
  loading: boolean;
  error: string | null;
  hasAnyModels: boolean;
  isFirstRun: boolean;
  initialized: boolean;

  // Actions
  initialize: () => Promise<void>;
  loadModels: () => Promise<void>;
  loadCurrentModel: () => Promise<void>;
  checkFirstRun: () => Promise<boolean>;
  selectModel: (modelId: string) => Promise<boolean>;
  downloadModel: (modelId: string) => Promise<boolean>;
  deleteModel: (modelId: string) => Promise<boolean>;
  getModelInfo: (modelId: string) => ModelInfo | undefined;
  isModelDownloading: (modelId: string) => boolean;
  isModelExtracting: (modelId: string) => boolean;
  getDownloadProgress: (modelId: string) => DownloadProgress | undefined;

  // Internal setters
  setModels: (models: ModelInfo[]) => void;
  setCurrentModel: (modelId: string) => void;
  setError: (error: string | null) => void;
  setLoading: (loading: boolean) => void;
}

export const useModelStore = create<ModelsStore>()(
  subscribeWithSelector((set, get) => ({
    models: [],
    currentModel: "",
    downloadingModels: new Set(),
    extractingModels: new Set(),
    downloadProgress: new Map(),
    downloadStats: new Map(),
    loading: true,
    error: null,
    hasAnyModels: false,
    isFirstRun: false,
    initialized: false,

    // Internal setters
    setModels: (models) => set({ models }),
    setCurrentModel: (currentModel) => set({ currentModel }),
    setError: (error) => set({ error }),
    setLoading: (loading) => set({ loading }),

    loadModels: async () => {
      try {
        const result = await commands.getAvailableModels();
        if (result.status === "ok") {
          set({ models: result.data, error: null });

          // Sync downloading state from backend
          const currentlyDownloading = new Set(
            result.data.filter((m) => m.is_downloading).map((m) => m.id),
          );
          set({ downloadingModels: currentlyDownloading });
        } else {
          set({ error: `Failed to load models: ${result.error}` });
        }
      } catch (err) {
        set({ error: `Failed to load models: ${err}` });
      } finally {
        set({ loading: false });
      }
    },

    loadCurrentModel: async () => {
      try {
        const result = await commands.getCurrentModel();
        if (result.status === "ok") {
          set({ currentModel: result.data });
        }
      } catch (err) {
        console.error("Failed to load current model:", err);
      }
    },

    checkFirstRun: async () => {
      try {
        const result = await commands.hasAnyModelsAvailable();
        if (result.status === "ok") {
          const hasModels = result.data;
          set({ hasAnyModels: hasModels, isFirstRun: !hasModels });
          return !hasModels;
        }
        return false;
      } catch (err) {
        console.error("Failed to check model availability:", err);
        return false;
      }
    },

    selectModel: async (modelId: string) => {
      try {
        set({ error: null });
        const result = await commands.setActiveModel(modelId);
        if (result.status === "ok") {
          set({
            currentModel: modelId,
            isFirstRun: false,
            hasAnyModels: true,
          });
          return true;
        } else {
          set({ error: `Failed to switch to model: ${result.error}` });
          return false;
        }
      } catch (err) {
        set({ error: `Failed to switch to model: ${err}` });
        return false;
      }
    },

    downloadModel: async (modelId: string) => {
      try {
        set({ error: null });
        set((state) => ({
          downloadingModels: new Set(state.downloadingModels).add(modelId),
        }));
        const result = await commands.downloadModel(modelId);
        if (result.status === "ok") {
          return true;
        } else {
          set({ error: `Failed to download model: ${result.error}` });
          set((state) => {
            const next = new Set(state.downloadingModels);
            next.delete(modelId);
            return { downloadingModels: next };
          });
          return false;
        }
      } catch (err) {
        set({ error: `Failed to download model: ${err}` });
        set((state) => {
          const next = new Set(state.downloadingModels);
          next.delete(modelId);
          return { downloadingModels: next };
        });
        return false;
      }
    },

    deleteModel: async (modelId: string) => {
      try {
        set({ error: null });
        const result = await commands.deleteModel(modelId);
        if (result.status === "ok") {
          await get().loadModels();
          return true;
        } else {
          set({ error: `Failed to delete model: ${result.error}` });
          return false;
        }
      } catch (err) {
        set({ error: `Failed to delete model: ${err}` });
        return false;
      }
    },

    getModelInfo: (modelId: string) => {
      return get().models.find((model) => model.id === modelId);
    },

    isModelDownloading: (modelId: string) => {
      return get().downloadingModels.has(modelId);
    },

    isModelExtracting: (modelId: string) => {
      return get().extractingModels.has(modelId);
    },

    getDownloadProgress: (modelId: string) => {
      return get().downloadProgress.get(modelId);
    },

    initialize: async () => {
      if (get().initialized) return;

      const { loadModels, loadCurrentModel, checkFirstRun } = get();

      // Load initial data
      await Promise.all([loadModels(), loadCurrentModel(), checkFirstRun()]);

      // Set up event listeners
      listen<DownloadProgress>("model-download-progress", (event) => {
        const progress = event.payload;
        set((state) => ({
          downloadProgress: new Map(state.downloadProgress).set(
            progress.model_id,
            progress,
          ),
        }));

        // Update download stats for speed calculation
        const now = Date.now();
        set((state) => {
          const current = state.downloadStats.get(progress.model_id);
          const newStats = new Map(state.downloadStats);

          if (!current) {
            newStats.set(progress.model_id, {
              startTime: now,
              lastUpdate: now,
              totalDownloaded: progress.downloaded,
              speed: 0,
            });
          } else {
            const timeDiff = (now - current.lastUpdate) / 1000;
            const bytesDiff = progress.downloaded - current.totalDownloaded;

            if (timeDiff > 0.5) {
              const currentSpeed = bytesDiff / (1024 * 1024) / timeDiff;
              const validCurrentSpeed = Math.max(0, currentSpeed);
              const smoothedSpeed =
                current.speed > 0
                  ? current.speed * 0.8 + validCurrentSpeed * 0.2
                  : validCurrentSpeed;

              newStats.set(progress.model_id, {
                startTime: current.startTime,
                lastUpdate: now,
                totalDownloaded: progress.downloaded,
                speed: Math.max(0, smoothedSpeed),
              });
            }
          }

          return { downloadStats: newStats };
        });
      });

      listen<string>("model-download-complete", (event) => {
        const modelId = event.payload;
        set((state) => {
          const nextDownloading = new Set(state.downloadingModels);
          nextDownloading.delete(modelId);
          const nextProgress = new Map(state.downloadProgress);
          nextProgress.delete(modelId);
          const nextStats = new Map(state.downloadStats);
          nextStats.delete(modelId);
          return {
            downloadingModels: nextDownloading,
            downloadProgress: nextProgress,
            downloadStats: nextStats,
          };
        });
        get().loadModels();
      });

      listen<string>("model-extraction-started", (event) => {
        const modelId = event.payload;
        set((state) => ({
          extractingModels: new Set(state.extractingModels).add(modelId),
        }));
      });

      listen<string>("model-extraction-completed", (event) => {
        const modelId = event.payload;
        set((state) => {
          const next = new Set(state.extractingModels);
          next.delete(modelId);
          return { extractingModels: next };
        });
        get().loadModels();
      });

      listen<{ model_id: string; error: string }>(
        "model-extraction-failed",
        (event) => {
          const modelId = event.payload.model_id;
          set((state) => {
            const next = new Set(state.extractingModels);
            next.delete(modelId);
            return {
              extractingModels: next,
              error: `Failed to extract model: ${event.payload.error}`,
            };
          });
        },
      );

      listen<string>("model-deleted", () => {
        get().loadModels();
        get().loadCurrentModel();
      });

      listen("model-state-changed", () => {
        get().loadModels();
        get().loadCurrentModel();
      });

      set({ initialized: true });
    },
  })),
);

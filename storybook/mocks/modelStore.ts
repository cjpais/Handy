import {
  MOCK_DOWNLOAD_PROGRESS,
  MOCK_DOWNLOAD_STATS,
  MOCK_MODELS,
} from "./data";

const store = {
  models: MOCK_MODELS,
  currentModel: "whisper-small",
  downloadingModels: { "whisper-large": true as const },
  extractingModels: {},
  downloadProgress: MOCK_DOWNLOAD_PROGRESS,
  downloadStats: MOCK_DOWNLOAD_STATS,
  loading: false,
  error: null as string | null,
  hasAnyModels: true,
  isFirstRun: false,
  initialized: true,
  initialize: async () => {},
  loadModels: async () => {},
  loadCurrentModel: async () => {},
  checkFirstRun: async () => false,
  selectModel: async (modelId: string) => {
    store.currentModel = modelId;
    return true;
  },
  downloadModel: async () => true,
  cancelDownload: async () => true,
  deleteModel: async () => true,
  getModelInfo: (modelId: string) =>
    store.models.find((model) => model.id === modelId),
  isModelDownloading: (modelId: string) => modelId in store.downloadingModels,
  isModelExtracting: (modelId: string) => modelId in store.extractingModels,
  getDownloadProgress: (modelId: string) => store.downloadProgress[modelId],
  setModels: () => {},
  setCurrentModel: () => {},
  setError: () => {},
  setLoading: () => {},
};

export const useModelStore = (selector?: (state: typeof store) => any) =>
  selector ? selector(store) : store;

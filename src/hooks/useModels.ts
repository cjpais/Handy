import { useEffect } from "react";
import { useModelsStore } from "@/stores/modelsStore";

export const useModels = () => {
  const store = useModelsStore();

  useEffect(() => {
    void store.initialize();
  }, []);

  return {
    models: store.models,
    currentModel: store.currentModel,
    loading: store.loading,
    error: store.error,
    downloadingModels: store.downloadingModels,
    extractingModels: store.extractingModels,
    downloadProgress: store.downloadProgress,
    downloadStats: store.downloadStats,
    hasAnyModels: store.hasAnyModels,
    isFirstRun: store.isFirstRun,
    loadModels: store.loadModels,
    loadCurrentModel: store.loadCurrentModel,
    checkFirstRun: store.checkFirstRun,
    selectModel: store.selectModel,
    downloadModel: store.downloadModel,
    cancelDownload: store.cancelDownload,
    deleteModel: store.deleteModel,
    getModelInfo: store.getModelInfo,
    isModelDownloading: store.isModelDownloading,
    isModelExtracting: store.isModelExtracting,
    getDownloadProgress: store.getDownloadProgress,
  };
};

import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

interface ModelInfo {
  id: string;
  name: string;
  description: string;
  filename: string;
  url?: string;
  size_mb: number;
  is_downloaded: boolean;
  is_downloading: boolean;
  partial_size: number;
  is_directory: boolean;
  accuracy_score: number;
  speed_score: number;
}

interface DownloadProgress {
  model_id: string;
  downloaded: number;
  total: number;
  percentage: number;
}

export const useModels = () => {
  const [models, setModels] = useState<ModelInfo[]>([]);
  const [currentModel, setCurrentModel] = useState<string>("");
  const [downloadingModels, setDownloadingModels] = useState<Set<string>>(
    new Set(),
  );
  const [extractingModels, setExtractingModels] = useState<Set<string>>(
    new Set(),
  );
  const [downloadProgress, setDownloadProgress] = useState<
    Map<string, DownloadProgress>
  >(new Map());
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [hasAnyModels, setHasAnyModels] = useState(false);
  const [isFirstRun, setIsFirstRun] = useState(false);

  const loadModels = async () => {
    try {
      const modelList = await invoke<ModelInfo[]>("get_available_models");
      setModels(modelList);
      setError(null);
    } catch (err) {
      setError(`Failed to load models: ${err}`);
    } finally {
      setLoading(false);
    }
  };

  const loadCurrentModel = async () => {
    try {
      const current = await invoke<string>("get_current_model");
      setCurrentModel(current);
    } catch (err) {
      console.error("Failed to load current model:", err);
    }
  };

  const checkFirstRun = async () => {
    try {
      const hasModels = await invoke<boolean>("has_any_models_available");
      setHasAnyModels(hasModels);
      setIsFirstRun(!hasModels);
      return !hasModels;
    } catch (err) {
      console.error("Failed to check model availability:", err);
      return false;
    }
  };

  const selectModel = async (modelId: string) => {
    try {
      setError(null);
      await invoke("set_active_model", { modelId });
      setCurrentModel(modelId);
      setIsFirstRun(false);
      setHasAnyModels(true);
      return true;
    } catch (err) {
      setError(`Failed to switch to model: ${err}`);
      return false;
    }
  };

  const downloadModel = async (modelId: string) => {
    try {
      setError(null);
      setDownloadingModels((prev) => new Set(prev.add(modelId)));
      await invoke("download_model", { modelId });
      return true;
    } catch (err) {
      setError(`Failed to download model: ${err}`);
      setDownloadingModels((prev) => {
        const next = new Set(prev);
        next.delete(modelId);
        return next;
      });
      return false;
    }
  };

  const deleteModel = async (modelId: string) => {
    try {
      setError(null);
      await invoke("delete_model", { modelId });
      await loadModels(); // Refresh the list
      return true;
    } catch (err) {
      setError(`Failed to delete model: ${err}`);
      return false;
    }
  };

  const getModelInfo = (modelId: string): ModelInfo | undefined => {
    return models.find((model) => model.id === modelId);
  };

  const isModelDownloading = (modelId: string): boolean => {
    return downloadingModels.has(modelId);
  };

  const isModelExtracting = (modelId: string): boolean => {
    return extractingModels.has(modelId);
  };

  const getDownloadProgress = (
    modelId: string,
  ): DownloadProgress | undefined => {
    return downloadProgress.get(modelId);
  };

  useEffect(() => {
    loadModels();
    loadCurrentModel();
    checkFirstRun();

    // Listen for download progress
    const progressUnlisten = listen<DownloadProgress>(
      "model-download-progress",
      (event) => {
        setDownloadProgress(
          (prev) => new Map(prev.set(event.payload.model_id, event.payload)),
        );
      },
    );

    // Listen for download completion
    const completeUnlisten = listen<string>(
      "model-download-complete",
      (event) => {
        const modelId = event.payload;
        setDownloadingModels((prev) => {
          const next = new Set(prev);
          next.delete(modelId);
          return next;
        });
        setDownloadProgress((prev) => {
          const next = new Map(prev);
          next.delete(modelId);
          return next;
        });
        // Refresh models list to update download status
        loadModels();
      },
    );

    // Listen for extraction events
    const extractionStartedUnlisten = listen<string>(
      "model-extraction-started",
      (event) => {
        const modelId = event.payload;
        setExtractingModels((prev) => new Set(prev.add(modelId)));
      },
    );

    const extractionCompletedUnlisten = listen<string>(
      "model-extraction-completed", 
      (event) => {
        const modelId = event.payload;
        setExtractingModels((prev) => {
          const next = new Set(prev);
          next.delete(modelId);
          return next;
        });
        // Refresh models list to update download status
        loadModels();
      },
    );

    const extractionFailedUnlisten = listen<{model_id: string, error: string}>(
      "model-extraction-failed",
      (event) => {
        const modelId = event.payload.model_id;
        setExtractingModels((prev) => {
          const next = new Set(prev);
          next.delete(modelId);
          return next;
        });
        setError(`Failed to extract model: ${event.payload.error}`);
      },
    );

    return () => {
      progressUnlisten.then((fn) => fn());
      completeUnlisten.then((fn) => fn());
      extractionStartedUnlisten.then((fn) => fn());
      extractionCompletedUnlisten.then((fn) => fn());
      extractionFailedUnlisten.then((fn) => fn());
    };
  }, []);

  return {
    models,
    currentModel,
    loading,
    error,
    downloadingModels,
    extractingModels,
    downloadProgress,
    hasAnyModels,
    isFirstRun,
    loadModels,
    loadCurrentModel,
    checkFirstRun,
    selectModel,
    downloadModel,
    deleteModel,
    getModelInfo,
    isModelDownloading,
    isModelExtracting,
    getDownloadProgress,
  };
};

import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { commands, type ModelInfo } from "@/bindings";

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
      const result = await commands.getAvailableModels();
      if (result.status === "ok") {
        setModels(result.data);
        setError(null);
      } else {
        setError(`Failed to load models: ${result.error}`);
      }
    } catch (err) {
      setError(`Failed to load models: ${err}`);
    } finally {
      setLoading(false);
    }
  };

  const loadCurrentModel = async () => {
    try {
      const result = await commands.getCurrentModel();
      if (result.status === "ok") {
        setCurrentModel(result.data);
      }
    } catch (err) {
      console.error("Failed to load current model:", err);
    }
  };

  const checkFirstRun = async () => {
    try {
      const result = await commands.hasAnyModelsAvailable();
      if (result.status === "ok") {
        const hasModels = result.data;
        setHasAnyModels(hasModels);
        setIsFirstRun(!hasModels);
        return !hasModels;
      }
      return false;
    } catch (err) {
      console.error("Failed to check model availability:", err);
      return false;
    }
  };

  const selectModel = async (modelId: string) => {
    try {
      setError(null);
      const result = await commands.setActiveModel(modelId);
      if (result.status === "ok") {
        setCurrentModel(modelId);
        setIsFirstRun(false);
        setHasAnyModels(true);
        return true;
      } else {
        setError(`Failed to switch to model: ${result.error}`);
        return false;
      }
    } catch (err) {
      setError(`Failed to switch to model: ${err}`);
      return false;
    }
  };

  const downloadModel = async (modelId: string) => {
    try {
      setError(null);
      setDownloadingModels((prev) => new Set(prev.add(modelId)));
      const result = await commands.downloadModel(modelId);
      if (result.status === "ok") {
        return true;
      } else {
        setError(`Failed to download model: ${result.error}`);
        setDownloadingModels((prev) => {
          const next = new Set(prev);
          next.delete(modelId);
          return next;
        });
        return false;
      }
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
      const result = await commands.deleteModel(modelId);
      if (result.status === "ok") {
        await loadModels(); // Refresh the list
        return true;
      } else {
        setError(`Failed to delete model: ${result.error}`);
        return false;
      }
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

    const extractionFailedUnlisten = listen<{
      model_id: string;
      error: string;
    }>("model-extraction-failed", (event) => {
      const modelId = event.payload.model_id;
      setExtractingModels((prev) => {
        const next = new Set(prev);
        next.delete(modelId);
        return next;
      });
      setError(`Failed to extract model: ${event.payload.error}`);
    });

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

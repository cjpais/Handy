import React, { useState, useRef, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { listen } from "@tauri-apps/api/event";
import { commands } from "@/bindings";
import { getTranslatedModelName } from "../../lib/utils/modelTranslation";
import { useModels } from "../../hooks/useModels";
import ModelStatusButton from "./ModelStatusButton";
import ModelDropdown from "./ModelDropdown";
import DownloadProgressDisplay from "./DownloadProgressDisplay";

interface ModelStateEvent {
  event_type: string;
  model_id?: string;
  model_name?: string;
  error?: string;
}

type ModelStatus =
  | "ready"
  | "loading"
  | "downloading"
  | "extracting"
  | "error"
  | "unloaded"
  | "none";

interface ModelSelectorProps {
  onError?: (error: string) => void;
}

const ModelSelector: React.FC<ModelSelectorProps> = ({ onError }) => {
  const { t } = useTranslation();
  const {
    models,
    currentModel,
    downloadProgress,
    downloadStats,
    extractingModels,
    downloadModel,
    cancelDownload,
    selectModel,
    deleteModel,
  } = useModels();

  // UI-specific state
  const [modelStatus, setModelStatus] = useState<ModelStatus>("unloaded");
  const [modelError, setModelError] = useState<string | null>(null);
  const [showModelDropdown, setShowModelDropdown] = useState(false);
  const [pendingAutoSelect, setPendingAutoSelect] = useState<string | null>(
    null,
  );

  const dropdownRef = useRef<HTMLDivElement>(null);

  // Check initial model status with retry for loading state
  useEffect(() => {
    const checkInitialStatus = async (retryCount = 0) => {
      if (currentModel) {
        const statusResult = await commands.getTranscriptionModelStatus();
        if (statusResult.status === "ok") {
          if (statusResult.data === currentModel) {
            setModelStatus("ready");
          } else if (statusResult.data === null && retryCount < 5) {
            // Model might still be loading, retry after a short delay
            setModelStatus("loading");
            setTimeout(() => checkInitialStatus(retryCount + 1), 1000);
          } else {
            setModelStatus("unloaded");
          }
        }
      } else {
        setModelStatus("none");
      }
    };
    checkInitialStatus();
  }, [currentModel]);

  // Listen for model state changes (UI-specific: tracks loading_started, loading_completed, etc.)
  useEffect(() => {
    const modelStateUnlisten = listen<ModelStateEvent>(
      "model-state-changed",
      (event) => {
        const { event_type, model_id, error } = event.payload;

        switch (event_type) {
          case "loading_started":
            setModelStatus("loading");
            setModelError(null);
            break;
          case "loading_completed":
            setModelStatus("ready");
            setModelError(null);
            break;
          case "loading_failed":
            setModelStatus("error");
            setModelError(error || "Failed to load model");
            break;
          case "unloaded":
            setModelStatus("unloaded");
            setModelError(null);
            break;
        }
      },
    );

    // Listen for extraction failed (UI-specific error handling)
    const extractionFailedUnlisten = listen<{
      model_id: string;
      error: string;
    }>("model-extraction-failed", (event) => {
      setModelError(`Failed to extract model: ${event.payload.error}`);
      setModelStatus("error");
    });

    // Auto-select newly downloaded/extracted models
    const downloadCompleteUnlisten = listen<string>(
      "model-download-complete",
      (event) => {
        setPendingAutoSelect(event.payload);
      },
    );

    const extractionCompletedUnlisten = listen<string>(
      "model-extraction-completed",
      (event) => {
        setPendingAutoSelect(event.payload);
      },
    );

    return () => {
      modelStateUnlisten.then((fn) => fn());
      extractionFailedUnlisten.then((fn) => fn());
      downloadCompleteUnlisten.then((fn) => fn());
      extractionCompletedUnlisten.then((fn) => fn());
    };
  }, []);

  // Handle auto-select after download/extraction completes
  useEffect(() => {
    if (!pendingAutoSelect) return;

    const autoSelect = async () => {
      const modelId = pendingAutoSelect;
      setPendingAutoSelect(null);

      // Skip auto-switch if recording in progress
      const isRecording = await commands.isRecording();
      if (isRecording) {
        return;
      }

      handleModelSelect(modelId);
    };

    const timer = setTimeout(autoSelect, 500);
    return () => clearTimeout(timer);
  }, [pendingAutoSelect]);

  // Click outside to close dropdown
  useEffect(() => {
    const handleClickOutside = (event: MouseEvent) => {
      if (
        dropdownRef.current &&
        !dropdownRef.current.contains(event.target as Node)
      ) {
        setShowModelDropdown(false);
      }
    };

    document.addEventListener("mousedown", handleClickOutside);
    return () => document.removeEventListener("mousedown", handleClickOutside);
  }, []);

  const handleModelSelect = async (modelId: string) => {
    try {
      setModelError(null);
      setShowModelDropdown(false);
      const success = await selectModel(modelId);
      if (!success) {
        setModelStatus("error");
        onError?.("Failed to switch model");
      }
    } catch (err) {
      const errorMsg = `${err}`;
      setModelError(errorMsg);
      setModelStatus("error");
      onError?.(errorMsg);
    }
  };

  const handleModelDownload = async (modelId: string) => {
    try {
      setModelError(null);
      const success = await downloadModel(modelId);
      if (!success) {
        setModelStatus("error");
        onError?.("Failed to download model");
      }
    } catch (err) {
      const errorMsg = `${err}`;
      setModelError(errorMsg);
      setModelStatus("error");
      onError?.(errorMsg);
    }
  };

  const handleModelCancel = async (modelId: string) => {
    try {
      setModelError(null);
      const success = await cancelDownload(modelId);
      if (!success) {
        onError?.("Failed to cancel download");
      }
    } catch (err) {
      const errorMsg = `Failed to cancel: ${err}`;
      setModelError(errorMsg);
      onError?.(errorMsg);
    }
  };

  const handleModelDelete = async (modelId: string) => {
    try {
      await deleteModel(modelId);
      setModelError(null);
    } catch (err) {
      console.error("Failed to delete model:", err);
    }
  };

  const getCurrentModelInfo = () => {
    return models.find((m) => m.id === currentModel);
  };

  const getModelDisplayText = (): string => {
    if (extractingModels.size > 0) {
      if (extractingModels.size === 1) {
        const [modelId] = Array.from(extractingModels);
        const model = models.find((m) => m.id === modelId);
        const modelName = model
          ? getTranslatedModelName(model, t)
          : t("modelSelector.extractingGeneric").replace("...", "");
        return t("modelSelector.extracting", { modelName });
      } else {
        return t("modelSelector.extractingMultiple", {
          count: extractingModels.size,
        });
      }
    }

    if (downloadProgress.size > 0) {
      if (downloadProgress.size === 1) {
        const [progress] = Array.from(downloadProgress.values());
        const percentage = Math.max(
          0,
          Math.min(100, Math.round(progress.percentage)),
        );
        return t("modelSelector.downloading", { percentage });
      } else {
        return t("modelSelector.downloadingMultiple", {
          count: downloadProgress.size,
        });
      }
    }

    const currentModelInfo = getCurrentModelInfo();

    switch (modelStatus) {
      case "ready":
        return currentModelInfo
          ? getTranslatedModelName(currentModelInfo, t)
          : t("modelSelector.modelReady");
      case "loading":
        return currentModelInfo
          ? t("modelSelector.loading", {
              modelName: getTranslatedModelName(currentModelInfo, t),
            })
          : t("modelSelector.loadingGeneric");
      case "extracting":
        return currentModelInfo
          ? t("modelSelector.extracting", {
              modelName: getTranslatedModelName(currentModelInfo, t),
            })
          : t("modelSelector.extractingGeneric");
      case "error":
        return modelError || t("modelSelector.modelError");
      case "unloaded":
        return currentModelInfo
          ? getTranslatedModelName(currentModelInfo, t)
          : t("modelSelector.modelUnloaded");
      case "none":
        return t("modelSelector.noModelDownloadRequired");
      default:
        return currentModelInfo
          ? getTranslatedModelName(currentModelInfo, t)
          : t("modelSelector.modelUnloaded");
    }
  };

  return (
    <>
      {/* Model Status and Switcher */}
      <div className="relative" ref={dropdownRef}>
        <ModelStatusButton
          status={modelStatus}
          displayText={getModelDisplayText()}
          isDropdownOpen={showModelDropdown}
          onClick={() => setShowModelDropdown(!showModelDropdown)}
        />

        {/* Model Dropdown */}
        {showModelDropdown && (
          <ModelDropdown
            models={models}
            currentModelId={currentModel}
            downloadProgress={downloadProgress}
            onModelSelect={handleModelSelect}
            onModelDownload={handleModelDownload}
            onModelCancel={handleModelCancel}
            onModelDelete={handleModelDelete}
            onError={onError}
          />
        )}
      </div>

      {/* Download Progress Bar for Models */}
      <DownloadProgressDisplay
        downloadProgress={downloadProgress}
        downloadStats={downloadStats}
      />
    </>
  );
};

export default ModelSelector;

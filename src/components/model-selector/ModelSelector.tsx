import React, { useState, useRef, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { listen } from "@tauri-apps/api/event";
import { produce } from "immer";
import { commands, type ModelInfo } from "@/bindings";
import { getTranslatedModelName } from "../../lib/utils/modelTranslation";
import ModelStatusButton from "./ModelStatusButton";
import ModelDropdown from "./ModelDropdown";
import DownloadProgressDisplay from "./DownloadProgressDisplay";

interface ModelStateEvent {
  event_type: string;
  model_id?: string;
  model_name?: string;
  error?: string;
}

interface DownloadProgress {
  model_id: string;
  downloaded: number;
  total: number;
  percentage: number;
}

type ModelStatus =
  | "ready"
  | "loading"
  | "downloading"
  | "extracting"
  | "error"
  | "unloaded"
  | "none";

interface DownloadStats {
  startTime: number;
  lastUpdate: number;
  totalDownloaded: number;
  speed: number;
}

interface ModelSelectorProps {
  onError?: (error: string) => void;
}

const ModelSelector: React.FC<ModelSelectorProps> = ({ onError }) => {
  const { t } = useTranslation();
  const [models, setModels] = useState<ModelInfo[]>([]);
  const [currentModelId, setCurrentModelId] = useState<string>("");
  const [modelStatus, setModelStatus] = useState<ModelStatus>("unloaded");
  const [modelError, setModelError] = useState<string | null>(null);
  const [modelDownloadProgress, setModelDownloadProgress] = useState<
    Record<string, DownloadProgress>
  >({});
  const [showModelDropdown, setShowModelDropdown] = useState(false);
  const [downloadStats, setDownloadStats] = useState<
    Record<string, DownloadStats>
  >({});
  const [extractingModels, setExtractingModels] = useState<
    Record<string, true>
  >({});

  const dropdownRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    loadModels();
    loadCurrentModel();

    // Listen for model state changes
    const modelStateUnlisten = listen<ModelStateEvent>(
      "model-state-changed",
      (event) => {
        const { event_type, model_id, model_name, error } = event.payload;

        switch (event_type) {
          case "loading_started":
            setModelStatus("loading");
            setModelError(null);
            break;
          case "loading_completed":
            setModelStatus("ready");
            setModelError(null);
            if (model_id) setCurrentModelId(model_id);
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

    // Listen for model download progress
    const downloadProgressUnlisten = listen<DownloadProgress>(
      "model-download-progress",
      (event) => {
        const progress = event.payload;
        setModelDownloadProgress(
          produce((downloadProgress) => {
            downloadProgress[progress.model_id] = progress;
          }),
        );
        setModelStatus("downloading");

        // Update download stats for speed calculation
        const now = Date.now();
        setDownloadStats(
          produce((stats) => {
            const current = stats[progress.model_id];

            if (!current) {
              // First progress update - initialize
              stats[progress.model_id] = {
                startTime: now,
                lastUpdate: now,
                totalDownloaded: progress.downloaded,
                speed: 0,
              };
            } else {
              // Calculate speed over last few seconds
              const timeDiff = (now - current.lastUpdate) / 1000; // seconds
              const bytesDiff = progress.downloaded - current.totalDownloaded;

              if (timeDiff > 0.5) {
                // Update speed every 500ms
                const currentSpeed = bytesDiff / (1024 * 1024) / timeDiff; // MB/s
                // Smooth the speed with exponential moving average, but ensure positive values
                const validCurrentSpeed = Math.max(0, currentSpeed);
                const smoothedSpeed =
                  current.speed > 0
                    ? current.speed * 0.8 + validCurrentSpeed * 0.2
                    : validCurrentSpeed;

                stats[progress.model_id] = {
                  startTime: current.startTime,
                  lastUpdate: now,
                  totalDownloaded: progress.downloaded,
                  speed: Math.max(0, smoothedSpeed),
                };
              }
            }
          }),
        );
      },
    );

    // Listen for model download completion
    const downloadCompleteUnlisten = listen<string>(
      "model-download-complete",
      (event) => {
        const modelId = event.payload;
        setModelDownloadProgress(
          produce((progress) => {
            delete progress[modelId];
          }),
        );
        setDownloadStats(
          produce((stats) => {
            delete stats[modelId];
          }),
        );
        loadModels(); // Refresh models list

        // Auto-select the newly downloaded model (skip if recording in progress)
        setTimeout(async () => {
          const isRecording = await commands.isRecording();
          if (isRecording) {
            return; // Skip auto-switch if recording in progress
          }
          loadCurrentModel();
          handleModelSelect(modelId);
        }, 500);
      },
    );

    // Listen for extraction events
    const extractionStartedUnlisten = listen<string>(
      "model-extraction-started",
      (event) => {
        const modelId = event.payload;
        setExtractingModels(
          produce((extracting) => {
            extracting[modelId] = true;
          }),
        );
        setModelStatus("extracting");
      },
    );

    const extractionCompletedUnlisten = listen<string>(
      "model-extraction-completed",
      (event) => {
        const modelId = event.payload;
        setExtractingModels(
          produce((extracting) => {
            delete extracting[modelId];
          }),
        );
        loadModels(); // Refresh models list

        // Auto-select the newly extracted model (skip if recording in progress)
        setTimeout(async () => {
          const isRecording = await commands.isRecording();
          if (isRecording) {
            return; // Skip auto-switch if recording in progress
          }
          loadCurrentModel();
          handleModelSelect(modelId);
        }, 500);
      },
    );

    const extractionFailedUnlisten = listen<{
      model_id: string;
      error: string;
    }>("model-extraction-failed", (event) => {
      const modelId = event.payload.model_id;
      setExtractingModels(
        produce((extracting) => {
          delete extracting[modelId];
        }),
      );
      setModelError(`Failed to extract model: ${event.payload.error}`);
      setModelStatus("error");
    });

    // Click outside to close dropdown
    const handleClickOutside = (event: MouseEvent) => {
      if (
        dropdownRef.current &&
        !dropdownRef.current.contains(event.target as Node)
      ) {
        setShowModelDropdown(false);
      }
    };

    document.addEventListener("mousedown", handleClickOutside);

    return () => {
      document.removeEventListener("mousedown", handleClickOutside);
      modelStateUnlisten.then((fn) => fn());
      downloadProgressUnlisten.then((fn) => fn());
      downloadCompleteUnlisten.then((fn) => fn());
      extractionStartedUnlisten.then((fn) => fn());
      extractionCompletedUnlisten.then((fn) => fn());
      extractionFailedUnlisten.then((fn) => fn());
    };
  }, []);

  const loadModels = async () => {
    try {
      const result = await commands.getAvailableModels();
      if (result.status === "ok") {
        setModels(result.data);
      }
    } catch (err) {
      console.error("Failed to load models:", err);
    }
  };

  const loadCurrentModel = async () => {
    try {
      const result = await commands.getCurrentModel();
      if (result.status === "ok") {
        const current = result.data;
        setCurrentModelId(current);

        if (current) {
          // Check if model is actually loaded
          const statusResult = await commands.getTranscriptionModelStatus();
          if (statusResult.status === "ok") {
            const transcriptionStatus = statusResult.data;
            if (transcriptionStatus === current) {
              setModelStatus("ready");
            } else {
              setModelStatus("unloaded");
            }
          }
        } else {
          setModelStatus("none");
        }
      }
    } catch (err) {
      console.error("Failed to load current model:", err);
      setModelStatus("error");
      setModelError("Failed to check model status");
    }
  };

  const handleModelSelect = async (modelId: string) => {
    try {
      setCurrentModelId(modelId); // Set optimistically so loading text shows correct model
      setModelError(null);
      setShowModelDropdown(false);
      const result = await commands.setActiveModel(modelId);
      if (result.status === "error") {
        const errorMsg = result.error;
        setModelError(errorMsg);
        setModelStatus("error");
        onError?.(errorMsg);
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
      const result = await commands.downloadModel(modelId);
      if (result.status === "error") {
        const errorMsg = result.error;
        setModelError(errorMsg);
        setModelStatus("error");
        onError?.(errorMsg);
      }
    } catch (err) {
      const errorMsg = `${err}`;
      setModelError(errorMsg);
      setModelStatus("error");
      onError?.(errorMsg);
    }
  };

  const getCurrentModel = () => {
    return models.find((m) => m.id === currentModelId);
  };

  const getModelDisplayText = (): string => {
    const extractingKeys = Object.keys(extractingModels);
    if (extractingKeys.length > 0) {
      if (extractingKeys.length === 1) {
        const modelId = extractingKeys[0];
        const model = models.find((m) => m.id === modelId);
        const modelName = model
          ? getTranslatedModelName(model, t)
          : t("modelSelector.extractingGeneric").replace("...", "");
        return t("modelSelector.extracting", { modelName });
      } else {
        return t("modelSelector.extractingMultiple", {
          count: extractingKeys.length,
        });
      }
    }

    const progressValues = Object.values(modelDownloadProgress);
    if (progressValues.length > 0) {
      if (progressValues.length === 1) {
        const progress = progressValues[0];
        const percentage = Math.max(
          0,
          Math.min(100, Math.round(progress.percentage)),
        );
        return t("modelSelector.downloading", { percentage });
      } else {
        return t("modelSelector.downloadingMultiple", {
          count: progressValues.length,
        });
      }
    }

    const currentModel = getCurrentModel();

    switch (modelStatus) {
      case "ready":
        return currentModel
          ? getTranslatedModelName(currentModel, t)
          : t("modelSelector.modelReady");
      case "loading":
        return currentModel
          ? t("modelSelector.loading", {
              modelName: getTranslatedModelName(currentModel, t),
            })
          : t("modelSelector.loadingGeneric");
      case "extracting":
        return currentModel
          ? t("modelSelector.extracting", {
              modelName: getTranslatedModelName(currentModel, t),
            })
          : t("modelSelector.extractingGeneric");
      case "error":
        return modelError || t("modelSelector.modelError");
      case "unloaded":
        return currentModel
          ? getTranslatedModelName(currentModel, t)
          : t("modelSelector.modelUnloaded");
      case "none":
        return t("modelSelector.noModelDownloadRequired");
      default:
        return currentModel
          ? getTranslatedModelName(currentModel, t)
          : t("modelSelector.modelUnloaded");
    }
  };

  const handleModelDelete = async (modelId: string) => {
    const result = await commands.deleteModel(modelId);
    if (result.status === "ok") {
      await loadModels();
      setModelError(null);
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
            currentModelId={currentModelId}
            downloadProgress={modelDownloadProgress}
            onModelSelect={handleModelSelect}
            onModelDownload={handleModelDownload}
            onModelDelete={handleModelDelete}
            onError={onError}
          />
        )}
      </div>

      {/* Download Progress Bar for Models */}
      <DownloadProgressDisplay
        downloadProgress={modelDownloadProgress}
        downloadStats={downloadStats}
      />
    </>
  );
};

export default ModelSelector;

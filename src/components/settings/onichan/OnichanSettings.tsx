import React, { useEffect, useState, useCallback, useRef } from "react";
import { useTranslation } from "react-i18next";
import { listen } from "@tauri-apps/api/event";
import { commands, OnichanModelInfo, OnichanMode } from "@/bindings";
import { SettingsGroup } from "../../ui/SettingsGroup";
import { AudioVisualizer } from "../live/AudioVisualizer";
import { useSettings } from "../../../hooks/useSettings";
import { useSidecarStore } from "@/stores/sidecarStore";
import {
  Bot,
  Mic,
  Trash2,
  Volume2,
  Download,
  Check,
  Loader2,
  Brain,
  MessageSquare,
  X,
  Cloud,
  HardDrive,
  Timer,
} from "lucide-react";

interface ConversationMessage {
  role: string;
  content: string;
}

interface OnichanState {
  status: string;
  message: string | null;
  mode: OnichanMode;
  local_llm_loaded: boolean;
  local_tts_loaded: boolean;
}

interface OnichanResponse {
  text: string;
  is_speaking: boolean;
}

interface DownloadProgress {
  model_id: string;
  downloaded: number;
  total: number;
  percentage: number;
}

interface PartialTranscription {
  text: string;
  filler_count: number;
  word_count: number;
  filler_percentage: number;
}

export const OnichanSettings: React.FC = () => {
  const { t } = useTranslation();
  const { settings } = useSettings();
  const [isEnabled, setIsEnabled] = useState(false);
  const [status, setStatus] = useState<string>("idle");
  const [statusMessage, setStatusMessage] = useState<string | null>(null);
  const [conversation, setConversation] = useState<ConversationMessage[]>([]);
  const [isRecording, setIsRecording] = useState(false);
  const [liveTranscription, setLiveTranscription] = useState<string>("");
  const [isTranscribing, setIsTranscribing] = useState(false);
  const messagesEndRef = useRef<HTMLDivElement>(null);
  const [mode, setMode] = useState<OnichanMode>("Cloud");
  const [isConversationMode, setIsConversationMode] = useState(false);
  const [conversationState, setConversationState] = useState<string>("idle");

  // Use global sidecar store for LLM/TTS loaded state
  const {
    llmLoaded: isLlmLoaded,
    ttsLoaded: isTtsLoaded,
    llmLoading,
    ttsLoading,
    onichanConversationRunning,
    setOnichanConversationRunning,
    refresh: refreshSidecarState,
  } = useSidecarStore();

  // Model state (local - for model selection UI)
  const [llmModels, setLlmModels] = useState<OnichanModelInfo[]>([]);
  const [ttsModels, setTtsModels] = useState<OnichanModelInfo[]>([]);
  const [selectedLlmId, setSelectedLlmId] = useState<string | null>(null);
  const [selectedTtsId, setSelectedTtsId] = useState<string | null>(null);
  const [downloadProgress, setDownloadProgress] = useState<Record<string, number>>({});
  const [loadingModel, setLoadingModel] = useState<string | null>(null);

  // Auto-scroll to bottom when new messages arrive
  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [conversation]);

  // Load models on mount
  useEffect(() => {
    const loadModels = async () => {
      const llm = await commands.getOnichanLlmModels();
      const tts = await commands.getOnichanTtsModels();
      setLlmModels(llm);
      setTtsModels(tts);
      // LLM/TTS loaded status comes from global sidecar store
    };
    loadModels();
  }, []);

  // Check initial state
  useEffect(() => {
    commands.onichanIsActive().then((active) => {
      setIsEnabled(active);
    });
    commands.onichanIsConversationRunning().then((running) => {
      setIsConversationMode(running);
    });
    commands.onichanGetHistory().then((history) => {
      setConversation(history);
    });
    commands.onichanGetMode().then((currentMode) => {
      setMode(currentMode);
    });
  }, []);

  // Listen for events
  useEffect(() => {
    const unlistenState = listen<OnichanState>("onichan-state", (event) => {
      setStatus(event.payload.status);
      setStatusMessage(event.payload.message);
      setMode(event.payload.mode);
      // LLM/TTS loaded state is managed by global sidecar store
    });

    const unlistenResponse = listen<OnichanResponse>(
      "onichan-response",
      (event) => {
        commands.onichanGetHistory().then((history) => {
          setConversation(history);
        });
      }
    );

    // Listen for conversation mode state changes
    const unlistenConversationState = listen<string>(
      "onichan-conversation-state",
      (event) => {
        setConversationState(event.payload);
        // Map conversation state to status for UI
        if (event.payload === "listening") {
          setStatus("listening");
          setIsRecording(false);
          setIsTranscribing(false);
        } else if (event.payload === "collecting") {
          setStatus("listening");
          setIsRecording(true);
        } else if (event.payload === "transcribing" || event.payload === "thinking") {
          setStatus("thinking");
          setIsTranscribing(true);
          setIsRecording(false);
        } else if (event.payload === "speaking") {
          setStatus("speaking");
          setIsTranscribing(false);
        }
      }
    );

    // Listen for user speech in conversation mode
    const unlistenUserSpeech = listen<string>(
      "onichan-user-speech",
      (event) => {
        setLiveTranscription(event.payload);
      }
    );

    const unlistenOverlay = listen<string>("show-overlay", (event) => {
      if (event.payload === "recording" && isEnabled) {
        setIsRecording(true);
        setIsTranscribing(false);
        setLiveTranscription("");
        setStatus("listening");
      } else if (event.payload === "transcribing" && isEnabled) {
        setIsRecording(false);
        setIsTranscribing(true);
        setStatus("thinking");
      }
    });

    const unlistenHide = listen("hide-overlay", () => {
      setIsRecording(false);
      setIsTranscribing(false);
    });

    // Listen for partial/live transcription updates
    const unlistenPartial = listen<PartialTranscription>(
      "partial-transcription",
      (event) => {
        if (isEnabled) {
          setLiveTranscription(event.payload.text);
        }
      }
    );

    const unlistenTranscription = listen<string>(
      "transcription-result",
      async (event) => {
        // Get fresh state from backend to avoid stale closure issues
        const currentSettings = await commands.getAppSettings();
        const isOnOnichanTab = currentSettings.active_ui_section === "onichan";
        const onichanActive = await commands.onichanIsActive();

        console.log("[Onichan] transcription-result received:", {
          text: event.payload,
          onichanActive,
          isOnOnichanTab,
          activeSection: currentSettings.active_ui_section,
        });

        if (onichanActive && isOnOnichanTab && event.payload.trim()) {
          console.log("[Onichan] Processing transcription...");
          setLiveTranscription(event.payload);
          setIsTranscribing(false);
          const result = await commands.onichanProcessInput(event.payload);
          console.log("[Onichan] LLM result:", result);
          if (result.status === "ok") {
            console.log("[Onichan] Speaking response...");
            await commands.onichanSpeak(result.data);
            console.log("[Onichan] Speak complete");
          }
        } else {
          console.log("[Onichan] Skipping - conditions not met");
        }
      }
    );

    // Listen for model download progress
    const unlistenProgress = listen<DownloadProgress>(
      "onichan-model-download-progress",
      (event) => {
        setDownloadProgress((prev) => ({
          ...prev,
          [event.payload.model_id]: event.payload.percentage,
        }));
      }
    );

    const unlistenComplete = listen<string>(
      "onichan-model-download-complete",
      async (event) => {
        setDownloadProgress((prev) => {
          const next = { ...prev };
          delete next[event.payload];
          return next;
        });
        // Refresh models
        const llm = await commands.getOnichanLlmModels();
        const tts = await commands.getOnichanTtsModels();
        setLlmModels(llm);
        setTtsModels(tts);
      }
    );

    return () => {
      unlistenState.then((fn) => fn());
      unlistenResponse.then((fn) => fn());
      unlistenConversationState.then((fn) => fn());
      unlistenUserSpeech.then((fn) => fn());
      unlistenOverlay.then((fn) => fn());
      unlistenHide.then((fn) => fn());
      unlistenPartial.then((fn) => fn());
      unlistenTranscription.then((fn) => fn());
      unlistenProgress.then((fn) => fn());
      unlistenComplete.then((fn) => fn());
    };
    // Note: transcription listener fetches fresh state from backend,
    // so no dependency on isEnabled or settings needed for it.
    // Other listeners still use isEnabled for local state updates.
  }, [isEnabled]);

  const handleToggle = useCallback(async () => {
    if (isEnabled) {
      // Stop conversation mode first if running
      if (isConversationMode) {
        await commands.onichanStopConversation();
        setIsConversationMode(false);
      }
      await commands.onichanDisable();
      setIsEnabled(false);
      setStatus("idle");
    } else {
      await commands.onichanEnable();
      setIsEnabled(true);
      // Start conversation mode automatically
      const result = await commands.onichanStartConversation();
      if (result.status === "ok") {
        setIsConversationMode(true);
      }
    }
  }, [isEnabled, isConversationMode]);

  const handleClearHistory = useCallback(async () => {
    await commands.onichanClearHistory();
    setConversation([]);
  }, []);

  const handleModeChange = useCallback(async (newMode: OnichanMode) => {
    await commands.onichanSetMode(newMode);
    setMode(newMode);
  }, []);

  const handleDownloadModel = useCallback(async (modelId: string) => {
    setDownloadProgress((prev) => ({ ...prev, [modelId]: 0 }));
    const result = await commands.downloadOnichanModel(modelId);
    if (result.status === "error") {
      setDownloadProgress((prev) => {
        const next = { ...prev };
        delete next[modelId];
        return next;
      });
    }
  }, []);

  const handleDeleteModel = useCallback(
    async (modelId: string) => {
      const result = await commands.deleteOnichanModel(modelId);
      if (result.status === "ok") {
        // Refresh models
        const llm = await commands.getOnichanLlmModels();
        const tts = await commands.getOnichanTtsModels();
        setLlmModels(llm);
        setTtsModels(tts);

        // Unload if this was the loaded model
        if (selectedLlmId === modelId) {
          await commands.unloadLocalLlm();
          setSelectedLlmId(null);
          refreshSidecarState();
        }
        if (selectedTtsId === modelId) {
          await commands.unloadLocalTts();
          setSelectedTtsId(null);
          refreshSidecarState();
        }
      }
    },
    [selectedLlmId, selectedTtsId, refreshSidecarState]
  );

  const handleLoadLlm = useCallback(async (modelId: string) => {
    setLoadingModel(modelId);
    try {
      console.log(`Loading LLM model: ${modelId}`);
      const result = await commands.loadLocalLlm(modelId);
      console.log(`Load LLM result:`, result);
      if (result.status === "ok") {
        setSelectedLlmId(modelId);
        refreshSidecarState();
      } else {
        console.error(`Failed to load LLM: ${result.error}`);
        alert(`Failed to load model: ${result.error}`);
      }
    } catch (error) {
      console.error(`Error loading LLM:`, error);
      alert(`Error loading model: ${error}`);
    } finally {
      setLoadingModel(null);
    }
  }, [refreshSidecarState]);

  const handleUnloadLlm = useCallback(async () => {
    await commands.unloadLocalLlm();
    setSelectedLlmId(null);
    refreshSidecarState();
  }, [refreshSidecarState]);

  const handleLoadTts = useCallback(async (modelId: string) => {
    setLoadingModel(modelId);
    const result = await commands.loadLocalTts(modelId);
    setLoadingModel(null);
    if (result.status === "ok") {
      setSelectedTtsId(modelId);
      refreshSidecarState();
    }
  }, [refreshSidecarState]);

  const handleUnloadTts = useCallback(async () => {
    await commands.unloadLocalTts();
    setSelectedTtsId(null);
    refreshSidecarState();
  }, [refreshSidecarState]);

  const getStatusIcon = () => {
    switch (status) {
      case "listening":
        return <Mic className="w-6 h-6 text-red-400 animate-pulse" />;
      case "thinking":
        return <Bot className="w-6 h-6 text-yellow-400 animate-spin" />;
      case "speaking":
        return <Volume2 className="w-6 h-6 text-green-400 animate-pulse" />;
      default:
        return <Bot className="w-6 h-6 text-text/60" />;
    }
  };

  const getStatusText = () => {
    switch (status) {
      case "listening":
        return t("onichan.status.listening");
      case "thinking":
        return t("onichan.status.thinking");
      case "speaking":
        return t("onichan.status.speaking");
      default:
        return t("onichan.status.idle");
    }
  };

  const renderModelCard = (
    model: OnichanModelInfo,
    isSelected: boolean,
    onLoad: () => void,
    onUnload: () => void
  ) => {
    const isDownloading = downloadProgress[model.id] !== undefined;
    const progress = downloadProgress[model.id] || 0;
    const isLoading = loadingModel === model.id;

    // Three states: Active (loaded), Available (downloaded), Download (not downloaded)
    const isActive = isSelected;
    const isAvailable = model.is_downloaded && !isSelected;
    const needsDownload = !model.is_downloaded;

    // Visual styling based on state
    const cardStyles = isActive
      ? "border-2 border-logo-primary bg-logo-primary/10 ring-1 ring-logo-primary/30"
      : isAvailable
        ? "border border-green-500/50 hover:border-green-500 cursor-pointer hover:bg-green-500/5"
        : "border border-background-dark/50 hover:border-text/20";

    return (
      <div
        key={model.id}
        className={`p-3 rounded-lg transition-all ${cardStyles}`}
        onClick={isAvailable && !isLoading ? onLoad : undefined}
      >
        <div className="flex items-start justify-between gap-2">
          <div className="flex-1 min-w-0">
            <div className="flex items-center gap-2 flex-wrap">
              <h4 className="font-medium text-sm truncate">{model.name}</h4>
              {/* Status Badge */}
              {isActive && (
                <span className="px-2 py-0.5 text-xs font-semibold bg-logo-primary text-background rounded-full flex items-center gap-1">
                  <Check className="w-3 h-3" />
                  {t("onichan.models.active")}
                </span>
              )}
              {isAvailable && !isLoading && (
                <span className="px-2 py-0.5 text-xs font-semibold bg-green-500/20 text-green-400 border border-green-500/30 rounded-full">
                  {t("onichan.models.available")}
                </span>
              )}
              {needsDownload && !isDownloading && (
                <span className="px-2 py-0.5 text-xs font-medium text-text/50 bg-background-dark/30 rounded-full">
                  {t("onichan.models.notDownloaded")}
                </span>
              )}
              {isLoading && (
                <span
                  className="px-2 py-0.5 text-xs font-medium text-yellow-400 bg-yellow-500/20 border border-yellow-500/30 rounded-full flex items-center gap-1"
                  title={t("onichan.models.loadingTooltip")}
                >
                  <Loader2 className="w-3 h-3 animate-spin" />
                  {t("onichan.models.loading")}
                </span>
              )}
            </div>
            <p className="text-xs text-text/60 mt-1">{model.description}</p>
            <p className="text-xs text-text/40 mt-1">
              {model.size_mb} MB
              {model.context_size && ` • ${model.context_size.toLocaleString()} ctx`}
              {model.voice_name && ` • ${model.voice_name}`}
            </p>
            {/* Hint for available models */}
            {isAvailable && !isLoading && (
              <p className="text-xs text-green-400/70 mt-1.5 italic">
                {t("onichan.models.clickToLoad")}
              </p>
            )}
          </div>

          <div className="flex items-center gap-1">
            {isDownloading ? (
              <div className="flex items-center gap-2">
                <div className="w-20 h-2 bg-background-dark rounded-full overflow-hidden">
                  <div
                    className="h-full bg-logo-primary transition-all"
                    style={{ width: `${progress}%` }}
                  />
                </div>
                <span className="text-xs text-text/60 w-10">
                  {Math.round(progress)}%
                </span>
              </div>
            ) : isActive ? (
              <button
                onClick={(e) => {
                  e.stopPropagation();
                  onUnload();
                }}
                className="px-2 py-1 rounded text-xs bg-red-500/20 text-red-400 hover:bg-red-500/30 transition-colors flex items-center gap-1"
                title={t("onichan.models.unload")}
              >
                <X className="w-3 h-3" />
                {t("onichan.models.unloadButton")}
              </button>
            ) : isAvailable ? (
              <button
                onClick={(e) => {
                  e.stopPropagation();
                  handleDeleteModel(model.id);
                }}
                className="p-1.5 rounded hover:bg-background-dark/50 text-text/40 hover:text-red-400 transition-colors"
                title={t("onichan.models.delete")}
              >
                <Trash2 className="w-4 h-4" />
              </button>
            ) : (
              <button
                onClick={() => handleDownloadModel(model.id)}
                className="px-3 py-1.5 rounded bg-logo-primary/20 text-logo-primary hover:bg-logo-primary/30 transition-colors text-xs font-medium flex items-center gap-1.5"
                title={t("onichan.models.download")}
              >
                <Download className="w-3.5 h-3.5" />
                {t("onichan.models.downloadButton")}
              </button>
            )}
          </div>
        </div>
      </div>
    );
  };

  return (
    <div className="flex flex-col gap-6 max-w-3xl w-full mx-auto">
      {/* Main Control */}
      <SettingsGroup title={t("onichan.title")}>
        <div className="p-4">
          <div className="flex items-center justify-between mb-4">
            <div className="flex items-center gap-3">
              {getStatusIcon()}
              <div>
                <p className="font-medium">{getStatusText()}</p>
                {statusMessage && (
                  <p className="text-sm text-text/60">{statusMessage}</p>
                )}
              </div>
            </div>
            <button
              onClick={handleToggle}
              disabled={!isEnabled && mode === "Local" && !isLlmLoaded}
              className={`px-4 py-2 rounded-lg font-medium transition-colors ${
                isEnabled
                  ? "bg-red-500/20 text-red-400 hover:bg-red-500/30"
                  : mode === "Local" && !isLlmLoaded
                    ? "bg-background-dark/30 text-text/30 cursor-not-allowed"
                    : "bg-logo-primary/20 text-logo-primary hover:bg-logo-primary/30"
              }`}
              title={
                isEnabled
                  ? t("onichan.tooltips.disableActive")
                  : mode === "Local" && !isLlmLoaded
                    ? t("onichan.tooltips.enableDisabled")
                    : t("onichan.tooltips.enableReady")
              }
            >
              {isEnabled ? t("onichan.disable") : t("onichan.enable")}
            </button>
          </div>

          {/* Mode Toggle */}
          <div className="flex items-center justify-between py-3 border-t border-background-dark/50">
            <div className="flex items-center gap-2">
              {mode === "Local" ? (
                <HardDrive className="w-4 h-4 text-text/60" />
              ) : (
                <Cloud className="w-4 h-4 text-text/60" />
              )}
              <span className="text-sm">{t("onichan.mode.label")}</span>
            </div>
            <div className="flex items-center gap-1 bg-background-dark/50 rounded-lg p-1">
              <button
                onClick={() => handleModeChange("Local")}
                className={`px-3 py-1.5 rounded-md text-sm font-medium transition-colors flex items-center gap-1.5 ${
                  mode === "Local"
                    ? "bg-logo-primary/20 text-logo-primary"
                    : "text-text/60 hover:text-text"
                }`}
                title={t("onichan.tooltips.modeLocal")}
              >
                <HardDrive className="w-3.5 h-3.5" />
                {t("onichan.mode.local")}
              </button>
              <button
                onClick={() => handleModeChange("Cloud")}
                className={`px-3 py-1.5 rounded-md text-sm font-medium transition-colors flex items-center gap-1.5 ${
                  mode === "Cloud"
                    ? "bg-logo-primary/20 text-logo-primary"
                    : "text-text/60 hover:text-text"
                }`}
                title={t("onichan.tooltips.modeCloud")}
              >
                <Cloud className="w-3.5 h-3.5" />
                {t("onichan.mode.cloud")}
              </button>
            </div>
          </div>

          {/* Mode-specific info */}
          {mode === "Local" && !isLlmLoaded && (
            <div className="mt-2 p-2 rounded-lg bg-yellow-500/10 border border-yellow-500/20">
              <p className="text-xs text-yellow-400">
                {t("onichan.mode.localWarning")}
              </p>
            </div>
          )}
          {mode === "Cloud" && (
            <p className="mt-2 text-xs text-text/50">
              {t("onichan.mode.cloudInfo")}
            </p>
          )}

          {/* Silence Threshold Slider */}
          <div className="mt-4 py-3 border-t border-background-dark/50">
            <div className="flex items-center justify-between mb-2">
              <div className="flex items-center gap-2">
                <Timer className="w-4 h-4 text-text/60" />
                <span className="text-sm">{t("onichan.silenceThreshold.label")}</span>
              </div>
              <span className="text-sm text-text/60">
                {t("onichan.silenceThreshold.value", {
                  value: ((settings?.onichan_silence_threshold ?? 1500) / 1000).toFixed(1),
                })}
              </span>
            </div>
            <input
              type="range"
              min="500"
              max="5000"
              step="100"
              value={settings?.onichan_silence_threshold ?? 1500}
              onChange={(e) => {
                const value = parseInt(e.target.value);
                commands.changeOnichanSilenceThresholdSetting(BigInt(value));
              }}
              className="w-full h-2 bg-background-dark rounded-lg appearance-none cursor-pointer accent-logo-primary"
            />
            <div className="flex justify-between text-xs text-text/40 mt-1">
              <span>{t("onichan.silenceThreshold.min")}</span>
              <span>{t("onichan.silenceThreshold.description")}</span>
              <span>{t("onichan.silenceThreshold.max")}</span>
            </div>
          </div>

          {/* TTS Volume Slider */}
          <div className="py-3 border-t border-background-dark/50">
            <div className="flex items-center justify-between mb-2">
              <div className="flex items-center gap-2">
                <Volume2 className="w-4 h-4 text-text/60" />
                <span className="text-sm">{t("onichan.volume.label")}</span>
              </div>
              <span className="text-sm text-text/60">
                {Math.round((settings?.audio_feedback_volume ?? 0.4) * 100)}%
              </span>
            </div>
            <input
              type="range"
              min="0"
              max="100"
              step="5"
              value={Math.round((settings?.audio_feedback_volume ?? 0.4) * 100)}
              onChange={(e) => {
                const value = parseInt(e.target.value) / 100;
                commands.changeAudioFeedbackVolumeSetting(value);
              }}
              className="w-full h-2 bg-background-dark rounded-lg appearance-none cursor-pointer accent-logo-primary"
            />
            <div className="flex justify-between text-xs text-text/40 mt-1">
              <span>{t("onichan.volume.min")}</span>
              <span>{t("onichan.volume.description")}</span>
              <span>{t("onichan.volume.max")}</span>
            </div>
          </div>

          {/* Audio Visualizer when recording */}
          {isEnabled && isRecording && (
            <div className="mt-4">
              <AudioVisualizer isRecording={isRecording} />
            </div>
          )}

          {/* Live Transcription Display */}
          {isEnabled && (isRecording || isTranscribing || liveTranscription) && (
            <div className="mt-4 p-3 bg-background-dark/30 rounded-lg">
              <div className="flex items-center gap-2 mb-2">
                <Mic className="w-4 h-4 text-logo-primary" />
                <span className="text-xs text-text/60 uppercase tracking-wide">
                  {t("onichan.liveTranscription")}
                </span>
                {(isRecording || isTranscribing) && (
                  <span className="flex h-2 w-2">
                    <span className="animate-ping absolute inline-flex h-2 w-2 rounded-full bg-logo-primary opacity-75"></span>
                    <span className="relative inline-flex rounded-full h-2 w-2 bg-logo-primary"></span>
                  </span>
                )}
              </div>
              <p className="text-sm text-text min-h-[1.5rem]">
                {liveTranscription || (
                  <span className="text-text/40 italic">
                    {isRecording ? t("onichan.listeningPlaceholder") : t("onichan.processingPlaceholder")}
                  </span>
                )}
              </p>
            </div>
          )}

          {/* Instructions */}
          {isEnabled && status === "idle" && !liveTranscription && (
            <p className="text-sm text-text/60 mt-4">
              {isConversationMode
                ? t("onichan.instructionsContinuous")
                : t("onichan.instructions")}
            </p>
          )}

          {/* Conversation mode indicator */}
          {isEnabled && isConversationMode && status === "listening" && (
            <div className="mt-4 p-3 bg-green-500/10 border border-green-500/30 rounded-lg flex items-center gap-2">
              <span className="relative flex h-3 w-3">
                <span className="animate-ping absolute inline-flex h-full w-full rounded-full bg-green-400 opacity-75"></span>
                <span className="relative inline-flex rounded-full h-3 w-3 bg-green-500"></span>
              </span>
              <span className="text-sm text-green-400">
                {t("onichan.continuousListening")}
              </span>
            </div>
          )}
        </div>
      </SettingsGroup>

      {/* LLM Model Selection */}
      <SettingsGroup title={t("onichan.models.llmTitle")}>
        <div className="p-4">
          <div className="flex items-center gap-2 mb-3">
            <Brain className="w-4 h-4 text-text/60" />
            <p className="text-sm text-text/60">
              {t("onichan.models.llmDescription")}
            </p>
          </div>
          <div className="flex flex-col gap-2">
            {llmModels.map((model) =>
              renderModelCard(
                model,
                selectedLlmId === model.id && isLlmLoaded,
                () => handleLoadLlm(model.id),
                handleUnloadLlm
              )
            )}
          </div>
        </div>
      </SettingsGroup>

      {/* TTS Model Selection */}
      <SettingsGroup title={t("onichan.models.ttsTitle")}>
        <div className="p-4">
          <div className="flex items-center gap-2 mb-3">
            <MessageSquare className="w-4 h-4 text-text/60" />
            <p className="text-sm text-text/60">
              {t("onichan.models.ttsDescription")}
            </p>
          </div>
          <div className="flex flex-col gap-2">
            {ttsModels.map((model) =>
              renderModelCard(
                model,
                selectedTtsId === model.id && isTtsLoaded,
                () => handleLoadTts(model.id),
                handleUnloadTts
              )
            )}
          </div>
          <p className="text-xs text-text/40 mt-3">
            {t("onichan.models.ttsNote")}
          </p>
        </div>
      </SettingsGroup>

      {/* Conversation History */}
      {conversation.length > 0 && (
        <SettingsGroup title={t("onichan.conversation")}>
          <div className="p-4">
            <div className="flex justify-end mb-2">
              <button
                onClick={handleClearHistory}
                className="p-1.5 rounded hover:bg-background-dark/50 text-text/60 hover:text-red-400 transition-colors flex items-center gap-1 text-xs"
                title={t("onichan.clearHistory")}
              >
                <Trash2 className="w-3 h-3" />
                {t("onichan.clearHistory")}
              </button>
            </div>
            <div className="max-h-[400px] overflow-y-auto">
              <div className="flex flex-col gap-3">
                {conversation.map((msg, index) => (
                  <div
                    key={index}
                    className={`flex ${
                      msg.role === "user" ? "justify-end" : "justify-start"
                    }`}
                  >
                    <div
                      className={`max-w-[80%] px-4 py-2 rounded-lg ${
                        msg.role === "user"
                          ? "bg-logo-primary/20 text-text"
                          : "bg-background-dark/50 text-text"
                      }`}
                    >
                      <p className="text-sm">{msg.content}</p>
                    </div>
                  </div>
                ))}
                <div ref={messagesEndRef} />
              </div>
            </div>
          </div>
        </SettingsGroup>
      )}

      {/* Setup Instructions when not configured */}
      {!isEnabled && conversation.length === 0 && (
        <div className="text-center text-text/60 py-8">
          <Bot className="w-16 h-16 mx-auto mb-4 text-text/30" />
          <p className="text-lg mb-2">{t("onichan.setupTitle")}</p>
          <p className="text-sm">{t("onichan.setupDescription")}</p>
        </div>
      )}
    </div>
  );
};

import React, { useEffect, useState, useCallback } from "react";
import { useTranslation } from "react-i18next";
import { commands, MemoryMessage, MemoryStatus, EmbeddingModelInfo } from "@/bindings";
import { SettingsGroup } from "../../ui/SettingsGroup";
import { useSidecarStore } from "@/stores/sidecarStore";
import {
  Database,
  Search,
  Trash2,
  RefreshCcw,
  CheckCircle,
  XCircle,
  Loader2,
  User,
  Bot,
  Brain,
  Check,
  Download,
  Play,
  Square,
} from "lucide-react";

export const MemorySettings: React.FC = () => {
  const { t } = useTranslation();

  // Use global sidecar store for memory state
  const {
    memoryRunning,
    memoryModelLoaded,
    memoryCount: globalMemoryCount,
    refresh: refreshSidecarState,
  } = useSidecarStore();

  const [status, setStatus] = useState<MemoryStatus | null>(null);
  const [memoryCount, setMemoryCount] = useState<number>(0);
  const [searchQuery, setSearchQuery] = useState("");
  const [searchResults, setSearchResults] = useState<MemoryMessage[]>([]);
  const [isSearching, setIsSearching] = useState(false);
  const [isClearing, setIsClearing] = useState(false);
  const [showClearConfirm, setShowClearConfirm] = useState(false);
  const [isLoading, setIsLoading] = useState(true);

  // Embedding model state
  const [embeddingModels, setEmbeddingModels] = useState<EmbeddingModelInfo[]>([]);
  const [currentModel, setCurrentModel] = useState<EmbeddingModelInfo | null>(null);
  const [isLoadingModel, setIsLoadingModel] = useState<string | null>(null);
  const [isStartingSidecar, setIsStartingSidecar] = useState(false);
  const [isStoppingSidecar, setIsStoppingSidecar] = useState(false);

  // Sync local status with global store state
  useEffect(() => {
    if (status) {
      setStatus((prev) =>
        prev
          ? { ...prev, is_running: memoryRunning, model_loaded: memoryModelLoaded }
          : prev
      );
    }
  }, [memoryRunning, memoryModelLoaded, status]);

  // Load initial status
  useEffect(() => {
    const loadStatus = async () => {
      setIsLoading(true);
      try {
        const statusResult = await commands.getMemoryStatus();
        if (statusResult.status === "ok") {
          setStatus(statusResult.data);
        }
        const countResult = await commands.getMemoryCount();
        if (countResult.status === "ok") {
          setMemoryCount(countResult.data);
        }
      } catch (e) {
        console.error("Failed to load memory status:", e);
      } finally {
        setIsLoading(false);
      }
    };
    loadStatus();
  }, []);

  // Load embedding models
  const loadEmbeddingModels = useCallback(async () => {
    try {
      const modelsResult = await commands.listEmbeddingModels();
      if (modelsResult.status === "ok") {
        setEmbeddingModels(modelsResult.data);
      }
      const currentResult = await commands.getCurrentEmbeddingModel();
      if (currentResult.status === "ok") {
        setCurrentModel(currentResult.data);
      }
    } catch (e) {
      console.error("Failed to load embedding models:", e);
    }
  }, []);

  // Load embedding models when status shows sidecar is running
  useEffect(() => {
    if (status?.is_running) {
      loadEmbeddingModels();
    }
  }, [status?.is_running, loadEmbeddingModels]);

  const handleSearch = useCallback(async () => {
    if (!searchQuery.trim()) {
      setSearchResults([]);
      return;
    }

    setIsSearching(true);
    try {
      const result = await commands.queryAllMemories(searchQuery, 20);
      if (result.status === "ok") {
        setSearchResults(result.data);
      }
    } catch (e) {
      console.error("Search failed:", e);
    } finally {
      setIsSearching(false);
    }
  }, [searchQuery]);

  const handleClearAll = useCallback(async () => {
    setIsClearing(true);
    try {
      const result = await commands.clearAllMemories();
      if (result.status === "ok") {
        setMemoryCount(0);
        setSearchResults([]);
        setShowClearConfirm(false);
      }
    } catch (e) {
      console.error("Clear failed:", e);
    } finally {
      setIsClearing(false);
    }
  }, []);

  const handleRefresh = useCallback(async () => {
    try {
      const countResult = await commands.getMemoryCount();
      if (countResult.status === "ok") {
        setMemoryCount(countResult.data);
      }
      const statusResult = await commands.getMemoryStatus();
      if (statusResult.status === "ok") {
        setStatus(statusResult.data);
        // If sidecar is now running, also load the embedding models
        if (statusResult.data.is_running) {
          await loadEmbeddingModels();
        }
      }
    } catch (e) {
      console.error("Refresh failed:", e);
    }
  }, [loadEmbeddingModels]);

  // Start the memory sidecar
  const handleStartSidecar = useCallback(async () => {
    setIsStartingSidecar(true);
    try {
      // listEmbeddingModels will auto-start the sidecar via ensure_sidecar()
      const modelsResult = await commands.listEmbeddingModels();
      if (modelsResult.status === "ok") {
        setEmbeddingModels(modelsResult.data);
        console.log("Loaded embedding models:", modelsResult.data);
      } else {
        console.error("Failed to list embedding models:", modelsResult.error);
      }
      // Refresh status after starting
      const statusResult = await commands.getMemoryStatus();
      if (statusResult.status === "ok") {
        setStatus(statusResult.data);
        console.log("Memory status:", statusResult.data);
      } else {
        console.error("Failed to get memory status:", statusResult.error);
      }
      const currentResult = await commands.getCurrentEmbeddingModel();
      if (currentResult.status === "ok") {
        setCurrentModel(currentResult.data);
      } else {
        console.error("Failed to get current model:", currentResult.error);
      }
      // Refresh global sidecar state
      refreshSidecarState();
    } catch (e) {
      console.error("Failed to start sidecar:", e);
    } finally {
      setIsStartingSidecar(false);
    }
  }, [refreshSidecarState]);

  // Stop the memory sidecar
  const handleStopSidecar = useCallback(async () => {
    setIsStoppingSidecar(true);
    try {
      const result = await commands.stopMemorySidecar();
      if (result.status === "ok") {
        setStatus({ is_running: false, model_loaded: false, total_memories: 0 });
        setEmbeddingModels([]);
        setCurrentModel(null);
        refreshSidecarState();
      }
    } catch (e) {
      console.error("Failed to stop sidecar:", e);
    } finally {
      setIsStoppingSidecar(false);
    }
  }, [refreshSidecarState]);

  const handleLoadModel = useCallback(async (modelId: string) => {
    setIsLoadingModel(modelId);
    try {
      const result = await commands.loadEmbeddingModel(modelId);
      if (result.status === "ok") {
        // Refresh model list to update is_loaded status
        await loadEmbeddingModels();
        refreshSidecarState();
      } else {
        console.error("Failed to load model:", result.error);
      }
    } catch (e) {
      console.error("Failed to load model:", e);
    } finally {
      setIsLoadingModel(null);
    }
  }, [loadEmbeddingModels, refreshSidecarState]);

  const formatTimestamp = (timestamp: number) => {
    const date = new Date(timestamp * 1000);
    return date.toLocaleDateString() + " " + date.toLocaleTimeString();
  };

  if (isLoading) {
    return (
      <div className="flex flex-col gap-6 max-w-3xl w-full mx-auto">
        <div className="flex items-center justify-center py-12">
          <Loader2 className="w-8 h-8 animate-spin text-text/60" />
        </div>
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-6 max-w-3xl w-full mx-auto">
      {/* Status Section */}
      <SettingsGroup title={t("memory.status.title")}>
        <div className="p-4">
          <div className="flex items-center justify-between mb-4">
            <div className="flex items-center gap-3">
              <Database className="w-6 h-6 text-text/60" />
              <div>
                <p className="font-medium">{t("memory.title")}</p>
                <p className="text-sm text-text/60">
                  {t("memory.status.memoryCount", { count: memoryCount })}
                </p>
              </div>
            </div>
            <button
              onClick={handleRefresh}
              className="p-2 rounded-lg hover:bg-background-dark/50 text-text/60 hover:text-text transition-colors"
              title={t("memory.status.refresh")}
            >
              <RefreshCcw className="w-5 h-5" />
            </button>
          </div>

          {/* Status indicators */}
          <div className="flex flex-col gap-2 mt-4 pt-4 border-t border-background-dark/50">
            <div className="flex items-center gap-2">
              {status?.is_running ? (
                <CheckCircle className="w-4 h-4 text-green-400" />
              ) : (
                <XCircle className="w-4 h-4 text-text/40" />
              )}
              <span className="text-sm text-text/80">
                {status?.is_running
                  ? t("memory.status.sidecarRunning")
                  : t("memory.status.sidecarStopped")}
              </span>
            </div>
            <div className="flex items-center gap-2">
              {status?.model_loaded ? (
                <CheckCircle className="w-4 h-4 text-green-400" />
              ) : (
                <XCircle className="w-4 h-4 text-text/40" />
              )}
              <span className="text-sm text-text/80">
                {status?.model_loaded
                  ? t("memory.status.modelLoaded")
                  : t("memory.status.modelNotLoaded")}
              </span>
            </div>
          </div>

          {/* Start/Stop button */}
          <div className="mt-4 pt-4 border-t border-background-dark/50">
            {!status?.is_running ? (
              <button
                onClick={handleStartSidecar}
                disabled={isStartingSidecar}
                className="w-full px-4 py-2.5 bg-logo-primary/20 text-logo-primary rounded-lg hover:bg-logo-primary/30 transition-colors disabled:opacity-50 flex items-center justify-center gap-2 font-medium"
              >
                {isStartingSidecar ? (
                  <>
                    <Loader2 className="w-4 h-4 animate-spin" />
                    {t("memory.status.starting")}
                  </>
                ) : (
                  <>
                    <Play className="w-4 h-4" />
                    {t("memory.status.startButton")}
                  </>
                )}
              </button>
            ) : (
              <button
                onClick={handleStopSidecar}
                disabled={isStoppingSidecar}
                className="w-full px-4 py-2.5 bg-red-500/20 text-red-400 rounded-lg hover:bg-red-500/30 transition-colors disabled:opacity-50 flex items-center justify-center gap-2 font-medium"
              >
                {isStoppingSidecar ? (
                  <>
                    <Loader2 className="w-4 h-4 animate-spin" />
                    {t("memory.status.stopping")}
                  </>
                ) : (
                  <>
                    <Square className="w-4 h-4" />
                    {t("memory.status.stopButton")}
                  </>
                )}
              </button>
            )}
          </div>

          {/* Info message */}
          <p className="text-xs text-text/50 mt-4">
            {t("memory.status.info")}
          </p>
        </div>
      </SettingsGroup>

      {/* Embedding Model Section */}
      <SettingsGroup title={t("memory.model.title")}>
        <div className="p-4">
          <div className="flex items-center gap-2 mb-3">
            <Brain className="w-4 h-4 text-text/60" />
            <p className="text-sm text-text/60">
              {t("memory.model.description")}
            </p>
          </div>
          <div className="flex flex-col gap-2">
            {embeddingModels.map((model) => {
              const isActive = model.is_loaded;
              const isAvailable = model.is_downloaded && !model.is_loaded;
              const needsDownload = !model.is_downloaded;
              const isLoading = isLoadingModel === model.id;

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
                  onClick={isAvailable && !isLoading ? () => handleLoadModel(model.id) : undefined}
                >
                  <div className="flex items-start justify-between gap-2">
                    <div className="flex-1 min-w-0">
                      <div className="flex items-center gap-2 flex-wrap">
                        <h4 className="font-medium text-sm truncate">{model.name}</h4>
                        {/* Status Badge */}
                        {isActive && (
                          <span className="px-2 py-0.5 text-xs font-semibold bg-logo-primary text-background rounded-full flex items-center gap-1">
                            <Check className="w-3 h-3" />
                            {t("memory.model.active")}
                          </span>
                        )}
                        {isAvailable && !isLoading && (
                          <span className="px-2 py-0.5 text-xs font-semibold bg-green-500/20 text-green-400 border border-green-500/30 rounded-full">
                            {t("memory.model.available")}
                          </span>
                        )}
                        {needsDownload && (
                          <span className="px-2 py-0.5 text-xs font-medium text-text/50 bg-background-dark/30 rounded-full">
                            {t("memory.model.notDownloaded")}
                          </span>
                        )}
                        {isLoading && (
                          <span className="px-2 py-0.5 text-xs font-medium text-yellow-400 bg-yellow-500/20 border border-yellow-500/30 rounded-full flex items-center gap-1">
                            <Loader2 className="w-3 h-3 animate-spin" />
                            {t("memory.model.loading")}
                          </span>
                        )}
                      </div>
                      <p className="text-xs text-text/60 mt-1">{model.description}</p>
                      <p className="text-xs text-text/40 mt-1">
                        {model.size_mb} MB • {model.dimension} dim
                      </p>
                      {/* Hint for available models */}
                      {isAvailable && !isLoading && (
                        <p className="text-xs text-green-400/70 mt-1.5 italic">
                          {t("memory.model.clickToLoad")}
                        </p>
                      )}
                    </div>

                    <div className="flex items-center gap-1">
                      {needsDownload && (
                        <button
                          onClick={() => handleLoadModel(model.id)}
                          disabled={isLoading}
                          className="px-3 py-1.5 rounded bg-logo-primary/20 text-logo-primary hover:bg-logo-primary/30 transition-colors text-xs font-medium flex items-center gap-1.5 disabled:opacity-50"
                          title={t("memory.model.downloadAndLoad")}
                        >
                          <Download className="w-3.5 h-3.5" />
                          {t("memory.model.downloadButton")}
                        </button>
                      )}
                    </div>
                  </div>
                </div>
              );
            })}
          </div>
          {embeddingModels.length === 0 && (
            <p className="text-center text-text/40 py-4">
              {t("memory.model.loadFirst")}
            </p>
          )}
          <p className="text-xs text-text/40 mt-3">
            {t("memory.model.note")}
          </p>
        </div>
      </SettingsGroup>

      {/* Search/Browse Section */}
      <SettingsGroup title={t("memory.browser.title")}>
        <div className="p-4">
          {/* Search bar */}
          <div className="flex gap-2 mb-4">
            <div className="relative flex-1">
              <Search className="absolute left-3 top-1/2 transform -translate-y-1/2 w-4 h-4 text-text/40" />
              <input
                type="text"
                value={searchQuery}
                onChange={(e) => setSearchQuery(e.target.value)}
                onKeyDown={(e) => e.key === "Enter" && handleSearch()}
                placeholder={t("memory.browser.search")}
                className="w-full pl-10 pr-4 py-2 bg-background-dark/50 border border-background-dark rounded-lg text-sm focus:outline-none focus:border-logo-primary/50"
              />
            </div>
            <button
              onClick={handleSearch}
              disabled={isSearching || !searchQuery.trim()}
              className="px-4 py-2 bg-logo-primary/20 text-logo-primary rounded-lg hover:bg-logo-primary/30 transition-colors disabled:opacity-50 disabled:cursor-not-allowed flex items-center gap-2"
            >
              {isSearching ? (
                <Loader2 className="w-4 h-4 animate-spin" />
              ) : (
                <Search className="w-4 h-4" />
              )}
              {t("memory.browser.searchButton")}
            </button>
          </div>

          {/* Search results */}
          {searchResults.length > 0 ? (
            <div className="flex flex-col gap-2 max-h-[400px] overflow-y-auto">
              {searchResults.map((memory) => (
                <div
                  key={memory.id}
                  className="p-3 bg-background-dark/30 rounded-lg"
                >
                  <div className="flex items-start gap-2">
                    {memory.is_bot ? (
                      <Bot className="w-4 h-4 text-logo-primary mt-0.5 shrink-0" />
                    ) : (
                      <User className="w-4 h-4 text-text/60 mt-0.5 shrink-0" />
                    )}
                    <div className="flex-1 min-w-0">
                      <p className="text-sm text-text">{memory.content}</p>
                      <div className="flex items-center gap-2 mt-1 text-xs text-text/40">
                        <span>{formatTimestamp(memory.timestamp)}</span>
                        <span className="select-none">|</span>
                        <span>
                          {t("memory.browser.userId", {
                            id: memory.user_id.slice(0, 8),
                          })}
                        </span>
                        {memory.similarity && (
                          <>
                            <span className="select-none">|</span>
                            <span>
                              {t("memory.browser.similarity", {
                                percent: (memory.similarity * 100).toFixed(0),
                              })}
                            </span>
                          </>
                        )}
                      </div>
                    </div>
                  </div>
                </div>
              ))}
            </div>
          ) : searchQuery && !isSearching ? (
            <p className="text-center text-text/50 py-8">
              {t("memory.browser.noResults")}
            </p>
          ) : (
            <p className="text-center text-text/40 py-8">
              {t("memory.browser.searchHint")}
            </p>
          )}
        </div>
      </SettingsGroup>

      {/* Danger Zone */}
      <SettingsGroup title={t("memory.dangerZone.title")}>
        <div className="p-4">
          <div className="flex items-center justify-between">
            <div>
              <p className="font-medium text-red-400">
                {t("memory.browser.clearAll")}
              </p>
              <p className="text-sm text-text/60">
                {t("memory.dangerZone.clearDescription")}
              </p>
            </div>
            {showClearConfirm ? (
              <div className="flex gap-2">
                <button
                  onClick={() => setShowClearConfirm(false)}
                  className="px-3 py-1.5 text-sm text-text/60 hover:text-text transition-colors"
                >
                  {t("memory.dangerZone.cancel")}
                </button>
                <button
                  onClick={handleClearAll}
                  disabled={isClearing}
                  className="px-3 py-1.5 bg-red-500/20 text-red-400 rounded-lg hover:bg-red-500/30 transition-colors disabled:opacity-50 flex items-center gap-2"
                >
                  {isClearing ? (
                    <Loader2 className="w-4 h-4 animate-spin" />
                  ) : (
                    <Trash2 className="w-4 h-4" />
                  )}
                  {t("memory.dangerZone.confirm")}
                </button>
              </div>
            ) : (
              <button
                onClick={() => setShowClearConfirm(true)}
                disabled={memoryCount === 0}
                className="px-3 py-1.5 bg-red-500/10 text-red-400 rounded-lg hover:bg-red-500/20 transition-colors disabled:opacity-50 disabled:cursor-not-allowed flex items-center gap-2"
              >
                <Trash2 className="w-4 h-4" />
                {t("memory.browser.clearAll")}
              </button>
            )}
          </div>
        </div>
      </SettingsGroup>

      {/* Setup hint when empty */}
      {memoryCount === 0 && (
        <div className="text-center text-text/60 py-8">
          <Database className="w-16 h-16 mx-auto mb-4 text-text/30" />
          <p className="text-lg mb-2">{t("memory.empty.title")}</p>
          <p className="text-sm">{t("memory.empty.description")}</p>
        </div>
      )}
    </div>
  );
};

import React, { useEffect, useState, useRef, useCallback } from "react";
import { useTranslation } from "react-i18next";
import { useSidecarStore } from "@/stores/sidecarStore";
import { commands } from "@/bindings";
import { Brain, Volume2, MessageCircle, Database, X } from "lucide-react";

type SidecarState = "online" | "offline" | "loading";

interface SidecarInfo {
  id: string;
  labelKey: string;
  icon: React.ComponentType<{ className?: string }>;
  state: SidecarState;
}

const getStatusColor = (state: SidecarState): string => {
  switch (state) {
    case "online":
      return "bg-green-400";
    case "loading":
      return "bg-yellow-400 animate-pulse";
    case "offline":
      return "bg-mid-gray/40";
    default:
      return "bg-mid-gray/40";
  }
};

export const SidecarStatus: React.FC = () => {
  const { t } = useTranslation();
  const [activePopover, setActivePopover] = useState<string | null>(null);
  const [llmModelName, setLlmModelName] = useState<string | undefined>();
  const popoverRef = useRef<HTMLDivElement>(null);

  // Use global sidecar store instead of local state
  const {
    llmLoaded,
    llmLoading,
    ttsLoaded,
    ttsLoading,
    discordConnected,
    discordInVoice,
    discordGuild,
    discordChannel,
    memoryRunning,
    memoryModelLoaded,
    memoryCount,
  } = useSidecarStore();

  // Derive states from store values
  const llmState: SidecarState = llmLoading
    ? "loading"
    : llmLoaded
      ? "online"
      : "offline";
  const ttsState: SidecarState = ttsLoading
    ? "loading"
    : ttsLoaded
      ? "online"
      : "offline";
  const discordState: SidecarState =
    discordConnected || discordInVoice ? "online" : "offline";
  const memoryState: SidecarState = memoryRunning ? "online" : "offline";

  // Fetch LLM model name when LLM is loaded (this is display-only, not state)
  const fetchModelName = useCallback(async () => {
    if (llmLoaded) {
      try {
        const modelName = await commands.getLocalLlmModelName();
        setLlmModelName(modelName ?? undefined);
      } catch {
        setLlmModelName(undefined);
      }
    } else {
      setLlmModelName(undefined);
    }
  }, [llmLoaded]);

  useEffect(() => {
    fetchModelName();
  }, [fetchModelName]);

  // Close popover when clicking outside
  useEffect(() => {
    const handleClickOutside = (event: MouseEvent) => {
      if (
        popoverRef.current &&
        !popoverRef.current.contains(event.target as Node)
      ) {
        setActivePopover(null);
      }
    };

    if (activePopover) {
      document.addEventListener("mousedown", handleClickOutside);
      return () =>
        document.removeEventListener("mousedown", handleClickOutside);
    }
  }, [activePopover]);

  const handleClick = (id: string) => {
    setActivePopover(activePopover === id ? null : id);
  };

  const renderPopoverContent = (id: string) => {
    switch (id) {
      case "llm":
        return (
          <div className="space-y-2">
            <div className="flex items-center justify-between">
              <span className="text-text/60">{t("footer.popover.status")}</span>
              <span className={llmLoaded ? "text-green-400" : "text-text/40"}>
                {llmLoaded
                  ? t("footer.popover.loaded")
                  : t("footer.popover.notLoaded")}
              </span>
            </div>
            {llmModelName && (
              <div className="flex items-center justify-between">
                <span className="text-text/60">
                  {t("footer.popover.model")}
                </span>
                <span className="text-text/80 truncate max-w-32">
                  {llmModelName}
                </span>
              </div>
            )}
            <p className="text-xs text-text/40 pt-2 border-t border-mid-gray/20">
              {t("footer.popover.llmDescription")}
            </p>
          </div>
        );

      case "tts":
        return (
          <div className="space-y-2">
            <div className="flex items-center justify-between">
              <span className="text-text/60">{t("footer.popover.status")}</span>
              <span className={ttsLoaded ? "text-green-400" : "text-text/40"}>
                {ttsLoaded
                  ? t("footer.popover.loaded")
                  : t("footer.popover.notLoaded")}
              </span>
            </div>
            <p className="text-xs text-text/40 pt-2 border-t border-mid-gray/20">
              {t("footer.popover.ttsDescription")}
            </p>
          </div>
        );

      case "discord":
        return (
          <div className="space-y-2">
            <div className="flex items-center justify-between">
              <span className="text-text/60">
                {t("footer.popover.connected")}
              </span>
              <span
                className={discordConnected ? "text-green-400" : "text-text/40"}
              >
                {discordConnected
                  ? t("footer.popover.yes")
                  : t("footer.popover.no")}
              </span>
            </div>
            <div className="flex items-center justify-between">
              <span className="text-text/60">
                {t("footer.popover.inVoice")}
              </span>
              <span
                className={discordInVoice ? "text-green-400" : "text-text/40"}
              >
                {discordInVoice
                  ? t("footer.popover.yes")
                  : t("footer.popover.no")}
              </span>
            </div>
            {discordGuild && (
              <div className="flex items-center justify-between">
                <span className="text-text/60">
                  {t("footer.popover.server")}
                </span>
                <span className="text-text/80 truncate max-w-32">
                  {discordGuild}
                </span>
              </div>
            )}
            {discordChannel && (
              <div className="flex items-center justify-between">
                <span className="text-text/60">
                  {t("footer.popover.channel")}
                </span>
                <span className="text-text/80 truncate max-w-32">
                  {discordChannel}
                </span>
              </div>
            )}
            <p className="text-xs text-text/40 pt-2 border-t border-mid-gray/20">
              {t("footer.popover.discordDescription")}
            </p>
          </div>
        );

      case "memory":
        return (
          <div className="space-y-2">
            <div className="flex items-center justify-between">
              <span className="text-text/60">
                {t("footer.popover.sidecar")}
              </span>
              <span
                className={memoryRunning ? "text-green-400" : "text-text/40"}
              >
                {memoryRunning
                  ? t("footer.popover.running")
                  : t("footer.popover.stopped")}
              </span>
            </div>
            <div className="flex items-center justify-between">
              <span className="text-text/60">
                {t("footer.popover.embeddingModel")}
              </span>
              <span
                className={memoryModelLoaded ? "text-green-400" : "text-text/40"}
              >
                {memoryModelLoaded
                  ? t("footer.popover.loaded")
                  : t("footer.popover.notLoaded")}
              </span>
            </div>
            {memoryRunning && (
              <div className="flex items-center justify-between">
                <span className="text-text/60">
                  {t("footer.popover.memories")}
                </span>
                <span className="text-text/80">{memoryCount}</span>
              </div>
            )}
            <p className="text-xs text-text/40 pt-2 border-t border-mid-gray/20">
              {t("footer.popover.memoryDescription")}
            </p>
          </div>
        );

      default:
        return null;
    }
  };

  const sidecars: SidecarInfo[] = [
    {
      id: "llm",
      labelKey: "footer.sidecar.llm",
      icon: Brain,
      state: llmState,
    },
    {
      id: "tts",
      labelKey: "footer.sidecar.tts",
      icon: Volume2,
      state: ttsState,
    },
    {
      id: "discord",
      labelKey: "footer.sidecar.discord",
      icon: MessageCircle,
      state: discordState,
    },
    {
      id: "memory",
      labelKey: "footer.sidecar.memory",
      icon: Database,
      state: memoryState,
    },
  ];

  return (
    <div className="flex items-center gap-2 relative" ref={popoverRef}>
      {sidecars.map((sidecar) => {
        const Icon = sidecar.icon;
        const isActive = activePopover === sidecar.id;

        return (
          <div key={sidecar.id} className="relative">
            <button
              onClick={() => handleClick(sidecar.id)}
              className={`flex items-center gap-1 transition-colors cursor-pointer p-1 rounded ${
                isActive
                  ? "text-text/90 bg-mid-gray/20"
                  : "text-text/50 hover:text-text/70"
              }`}
            >
              <Icon className="w-3 h-3" />
              <div
                className={`w-1.5 h-1.5 rounded-full ${getStatusColor(sidecar.state)}`}
              />
            </button>

            {/* Popover */}
            {isActive && (
              <div className="absolute bottom-full left-1/2 -translate-x-1/2 mb-2 w-56 bg-background border border-mid-gray/30 rounded-lg shadow-xl z-50">
                {/* Header */}
                <div className="flex items-center justify-between px-3 py-2 border-b border-mid-gray/20">
                  <div className="flex items-center gap-2">
                    <Icon className="w-4 h-4 text-text/70" />
                    <span className="font-medium text-sm">
                      {t(sidecar.labelKey)}
                    </span>
                  </div>
                  <button
                    onClick={() => setActivePopover(null)}
                    className="text-text/40 hover:text-text/60 transition-colors"
                  >
                    <X className="w-3.5 h-3.5" />
                  </button>
                </div>

                {/* Content */}
                <div className="px-3 py-2 text-xs">
                  {renderPopoverContent(sidecar.id)}
                </div>

                {/* Arrow */}
                <div className="absolute top-full left-1/2 -translate-x-1/2 -mt-px">
                  <div className="w-2 h-2 bg-background border-r border-b border-mid-gray/30 transform rotate-45" />
                </div>
              </div>
            )}
          </div>
        );
      })}
    </div>
  );
};

export default SidecarStatus;

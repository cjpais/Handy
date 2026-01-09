import React, { useEffect, useState, useCallback, useRef } from "react";
import { useTranslation } from "react-i18next";
import { commands } from "@/bindings";
import { Brain, Volume2, MessageCircle, Database, X } from "lucide-react";

type SidecarState = "online" | "offline" | "loading";

interface SidecarDetails {
  llm: {
    loaded: boolean;
    modelName?: string;
  };
  tts: {
    loaded: boolean;
  };
  discord: {
    connected: boolean;
    inVoice: boolean;
    guildName?: string;
    channelName?: string;
  };
  memory: {
    running: boolean;
    modelLoaded: boolean;
    memoryCount: number;
  };
}

interface SidecarInfo {
  id: keyof SidecarDetails;
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
  const [llmState, setLlmState] = useState<SidecarState>("offline");
  const [ttsState, setTtsState] = useState<SidecarState>("offline");
  const [discordState, setDiscordState] = useState<SidecarState>("offline");
  const [memoryState, setMemoryState] = useState<SidecarState>("offline");
  const [activePopover, setActivePopover] = useState<string | null>(null);
  const [details, setDetails] = useState<SidecarDetails>({
    llm: { loaded: false },
    tts: { loaded: false },
    discord: { connected: false, inVoice: false },
    memory: { running: false, modelLoaded: false, memoryCount: 0 },
  });
  const popoverRef = useRef<HTMLDivElement>(null);

  const checkStatus = useCallback(async () => {
    try {
      // Check LLM status
      const llmLoaded = await commands.isLocalLlmLoaded();
      setLlmState(llmLoaded ? "online" : "offline");

      // Get current model name if loaded
      let modelName: string | undefined;
      if (llmLoaded) {
        try {
          modelName = await commands.getCurrentModel();
        } catch {
          // Ignore errors getting model name
        }
      }

      setDetails((prev) => ({
        ...prev,
        llm: { loaded: llmLoaded, modelName },
      }));

      // Check TTS status
      const ttsLoaded = await commands.isLocalTtsLoaded();
      setTtsState(ttsLoaded ? "online" : "offline");
      setDetails((prev) => ({
        ...prev,
        tts: { loaded: ttsLoaded },
      }));

      // Check Discord status - it's an object with connected/in_voice fields
      const discordStatus = await commands.discordGetStatus();
      setDiscordState(
        discordStatus.connected || discordStatus.in_voice ? "online" : "offline"
      );
      setDetails((prev) => ({
        ...prev,
        discord: {
          connected: discordStatus.connected,
          inVoice: discordStatus.in_voice,
          guildName: discordStatus.guild_name ?? undefined,
          channelName: discordStatus.channel_name ?? undefined,
        },
      }));

      // Check Memory status
      const memoryStatus = await commands.getMemoryStatus();
      if (memoryStatus.status === "ok") {
        setMemoryState(memoryStatus.data.is_running ? "online" : "offline");

        // Get memory count if running
        let memoryCount = 0;
        if (memoryStatus.data.is_running) {
          try {
            const countResult = await commands.getMemoryCount();
            if (countResult.status === "ok") {
              memoryCount = countResult.data;
            }
          } catch {
            // Ignore errors getting count
          }
        }

        setDetails((prev) => ({
          ...prev,
          memory: {
            running: memoryStatus.data.is_running,
            modelLoaded: memoryStatus.data.model_loaded,
            memoryCount,
          },
        }));
      }
    } catch (e) {
      console.error("Failed to check sidecar status:", e);
    }
  }, []);

  // Initial check and periodic polling
  useEffect(() => {
    checkStatus();
    const interval = setInterval(checkStatus, 5000); // Check every 5 seconds
    return () => clearInterval(interval);
  }, [checkStatus]);

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
              <span
                className={
                  details.llm.loaded ? "text-green-400" : "text-text/40"
                }
              >
                {details.llm.loaded
                  ? t("footer.popover.loaded")
                  : t("footer.popover.notLoaded")}
              </span>
            </div>
            {details.llm.modelName && (
              <div className="flex items-center justify-between">
                <span className="text-text/60">
                  {t("footer.popover.model")}
                </span>
                <span className="text-text/80 truncate max-w-32">
                  {details.llm.modelName}
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
              <span
                className={
                  details.tts.loaded ? "text-green-400" : "text-text/40"
                }
              >
                {details.tts.loaded
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
                className={
                  details.discord.connected ? "text-green-400" : "text-text/40"
                }
              >
                {details.discord.connected
                  ? t("footer.popover.yes")
                  : t("footer.popover.no")}
              </span>
            </div>
            <div className="flex items-center justify-between">
              <span className="text-text/60">
                {t("footer.popover.inVoice")}
              </span>
              <span
                className={
                  details.discord.inVoice ? "text-green-400" : "text-text/40"
                }
              >
                {details.discord.inVoice
                  ? t("footer.popover.yes")
                  : t("footer.popover.no")}
              </span>
            </div>
            {details.discord.guildName && (
              <div className="flex items-center justify-between">
                <span className="text-text/60">
                  {t("footer.popover.server")}
                </span>
                <span className="text-text/80 truncate max-w-32">
                  {details.discord.guildName}
                </span>
              </div>
            )}
            {details.discord.channelName && (
              <div className="flex items-center justify-between">
                <span className="text-text/60">
                  {t("footer.popover.channel")}
                </span>
                <span className="text-text/80 truncate max-w-32">
                  {details.discord.channelName}
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
                className={
                  details.memory.running ? "text-green-400" : "text-text/40"
                }
              >
                {details.memory.running
                  ? t("footer.popover.running")
                  : t("footer.popover.stopped")}
              </span>
            </div>
            <div className="flex items-center justify-between">
              <span className="text-text/60">
                {t("footer.popover.embeddingModel")}
              </span>
              <span
                className={
                  details.memory.modelLoaded ? "text-green-400" : "text-text/40"
                }
              >
                {details.memory.modelLoaded
                  ? t("footer.popover.loaded")
                  : t("footer.popover.notLoaded")}
              </span>
            </div>
            {details.memory.running && (
              <div className="flex items-center justify-between">
                <span className="text-text/60">
                  {t("footer.popover.memories")}
                </span>
                <span className="text-text/80">{details.memory.memoryCount}</span>
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

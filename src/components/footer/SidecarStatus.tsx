import React, { useEffect, useState, useCallback } from "react";
import { useTranslation } from "react-i18next";
import { commands } from "@/bindings";
import { Brain, Volume2, MessageCircle, Database } from "lucide-react";

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
  const [llmState, setLlmState] = useState<SidecarState>("offline");
  const [ttsState, setTtsState] = useState<SidecarState>("offline");
  const [discordState, setDiscordState] = useState<SidecarState>("offline");
  const [memoryState, setMemoryState] = useState<SidecarState>("offline");

  const checkStatus = useCallback(async () => {
    try {
      // Check LLM status
      const llmLoaded = await commands.isLocalLlmLoaded();
      setLlmState(llmLoaded ? "online" : "offline");

      // Check TTS status
      const ttsLoaded = await commands.isLocalTtsLoaded();
      setTtsState(ttsLoaded ? "online" : "offline");

      // Check Discord status - it's an object with connected/in_voice fields
      const discordStatus = await commands.discordGetStatus();
      setDiscordState(
        discordStatus.connected || discordStatus.in_voice
          ? "online"
          : "offline"
      );

      // Check Memory status
      const memoryStatus = await commands.getMemoryStatus();
      if (memoryStatus.status === "ok") {
        setMemoryState(memoryStatus.data.is_running ? "online" : "offline");
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

  const getTooltip = (labelKey: string, state: SidecarState): string => {
    const label = String(t(labelKey));
    switch (state) {
      case "online":
        return String(t("footer.sidecar.online", { name: label }));
      case "loading":
        return String(t("footer.sidecar.loading", { name: label }));
      case "offline":
        return String(t("footer.sidecar.offline", { name: label }));
      default:
        return label;
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
    <div className="flex items-center gap-2">
      {sidecars.map((sidecar) => {
        const Icon = sidecar.icon;
        return (
          <div
            key={sidecar.id}
            className="flex items-center gap-1 text-text/50 hover:text-text/70 transition-colors cursor-default"
            title={getTooltip(sidecar.labelKey, sidecar.state)}
          >
            <Icon className="w-3 h-3" />
            <div
              className={`w-1.5 h-1.5 rounded-full ${getStatusColor(sidecar.state)}`}
            />
          </div>
        );
      })}
    </div>
  );
};

export default SidecarStatus;

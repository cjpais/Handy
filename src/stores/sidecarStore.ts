import { create } from "zustand";
import { subscribeWithSelector } from "zustand/middleware";
import { commands } from "@/bindings";
import { listen, UnlistenFn } from "@tauri-apps/api/event";

export interface DiscordState {
  connected: boolean;
  in_voice: boolean;
  listening: boolean;
  guild_name: string | null;
  channel_name: string | null;
  error: string | null;
}

export interface OnichanState {
  active: boolean;
  mode: string;
  local_llm_loaded: boolean;
  local_tts_loaded: boolean;
}

export interface MemoryState {
  is_running: boolean;
  model_loaded: boolean;
  total_memories: number;
}

interface SidecarStore {
  // LLM state
  llmLoaded: boolean;
  llmModelName: string | null;
  llmLoading: boolean;

  // TTS state
  ttsLoaded: boolean;
  ttsLoading: boolean;

  // Discord state
  discordConnected: boolean;
  discordInVoice: boolean;
  discordGuild: string | null;
  discordChannel: string | null;
  discordConversationRunning: boolean;

  // Onichan state
  onichanActive: boolean;
  onichanMode: string;
  onichanConversationRunning: boolean;

  // Memory sidecar state
  memoryRunning: boolean;
  memoryModelLoaded: boolean;
  memoryModelId: string | null;
  memoryCount: number;

  // Actions
  initialize: () => Promise<void>;
  refresh: () => Promise<void>;
  cleanup: () => void;

  // LLM actions
  loadLlm: (modelId: string) => Promise<void>;
  unloadLlm: () => Promise<void>;
  setLlmLoading: (loading: boolean) => void;

  // TTS actions
  loadTts: (modelId: string) => Promise<void>;
  unloadTts: () => Promise<void>;
  setTtsLoading: (loading: boolean) => void;

  // Discord actions
  updateDiscordState: (state: Partial<DiscordState>) => void;
  setDiscordConversationRunning: (running: boolean) => void;

  // Onichan actions
  updateOnichanState: (state: Partial<OnichanState>) => void;
  setOnichanConversationRunning: (running: boolean) => void;

  // Memory actions
  updateMemoryState: (state: Partial<MemoryState>) => void;

  // Internal
  _unlisteners: UnlistenFn[];
  _setUnlisteners: (unlisteners: UnlistenFn[]) => void;
}

export const useSidecarStore = create<SidecarStore>()(
  subscribeWithSelector((set, get) => ({
    // Initial state
    llmLoaded: false,
    llmModelName: null,
    llmLoading: false,

    ttsLoaded: false,
    ttsLoading: false,

    discordConnected: false,
    discordInVoice: false,
    discordGuild: null,
    discordChannel: null,
    discordConversationRunning: false,

    onichanActive: false,
    onichanMode: "local",
    onichanConversationRunning: false,

    memoryRunning: false,
    memoryModelLoaded: false,
    memoryModelId: null,
    memoryCount: 0,

    _unlisteners: [],
    _setUnlisteners: (unlisteners) => set({ _unlisteners: unlisteners }),

    // Initialize: fetch current state and set up event listeners
    initialize: async () => {
      const { refresh, _setUnlisteners } = get();

      // First, fetch current state from backend
      await refresh();

      // Set up event listeners for state changes
      const unlisteners: UnlistenFn[] = [];

      // Listen for onichan state changes
      const unlistenOnichan = await listen<OnichanState>(
        "onichan-state",
        (event) => {
          set({
            onichanActive: event.payload.active,
            onichanMode: event.payload.mode,
            llmLoaded: event.payload.local_llm_loaded,
            ttsLoaded: event.payload.local_tts_loaded,
          });
        }
      );
      unlisteners.push(unlistenOnichan);

      // Listen for discord state changes
      const unlistenDiscord = await listen<DiscordState>(
        "discord-state",
        (event) => {
          set({
            discordConnected: event.payload.connected,
            discordInVoice: event.payload.in_voice,
            discordGuild: event.payload.guild_name ?? null,
            discordChannel: event.payload.channel_name ?? null,
          });
        }
      );
      unlisteners.push(unlistenDiscord);

      // Listen for memory state changes
      const unlistenMemory = await listen<MemoryState>(
        "memory-status",
        (event) => {
          set({
            memoryRunning: event.payload.is_running,
            memoryModelLoaded: event.payload.model_loaded,
            memoryModelId: null, // MemoryState doesn't include model ID
            memoryCount: event.payload.total_memories,
          });
        }
      );
      unlisteners.push(unlistenMemory);

      // Listen for model download completion (triggers refresh)
      const unlistenDownload = await listen(
        "onichan-model-download-complete",
        () => {
          refresh();
        }
      );
      unlisteners.push(unlistenDownload);

      _setUnlisteners(unlisteners);
    },

    // Refresh all state from backend
    refresh: async () => {
      try {
        // Fetch LLM/TTS state
        const llmLoaded = await commands.isLocalLlmLoaded();
        const ttsLoaded = await commands.isLocalTtsLoaded();

        // Fetch Discord state
        const discordStatus = await commands.discordGetStatus();
        const discordConversationRunning =
          await commands.discordIsConversationRunning();

        // Fetch Onichan state
        const onichanActive = await commands.onichanIsActive();
        const onichanMode = await commands.onichanGetMode();
        const onichanConversationRunning =
          await commands.onichanIsConversationRunning();

        // Fetch Memory state
        const memoryStatusResult = await commands.getMemoryStatus();
        const memoryStatus =
          memoryStatusResult.status === "ok" ? memoryStatusResult.data : null;

        set({
          llmLoaded,
          ttsLoaded,
          discordConnected: discordStatus.connected,
          discordInVoice: discordStatus.in_voice,
          discordGuild: discordStatus.guild_name ?? null,
          discordChannel: discordStatus.channel_name ?? null,
          discordConversationRunning,
          onichanActive,
          onichanMode,
          onichanConversationRunning,
          memoryRunning: memoryStatus?.is_running ?? false,
          memoryModelLoaded: memoryStatus?.model_loaded ?? false,
          memoryModelId: null, // MemoryStatus doesn't include model ID
        });
      } catch (error) {
        console.error("Failed to refresh sidecar state:", error);
      }
    },

    // Cleanup event listeners
    cleanup: () => {
      const { _unlisteners } = get();
      _unlisteners.forEach((unlisten) => unlisten());
      set({ _unlisteners: [] });
    },

    // LLM actions
    loadLlm: async (modelId: string) => {
      set({ llmLoading: true });
      try {
        const result = await commands.loadLocalLlm(modelId);
        if (result.status === "ok") {
          set({ llmLoaded: true, llmModelName: modelId });
        } else {
          console.error("Failed to load LLM:", result.error);
        }
      } catch (error) {
        console.error("Failed to load LLM:", error);
      } finally {
        set({ llmLoading: false });
      }
    },

    unloadLlm: async () => {
      try {
        await commands.unloadLocalLlm();
        set({ llmLoaded: false, llmModelName: null });
      } catch (error) {
        console.error("Failed to unload LLM:", error);
      }
    },

    setLlmLoading: (loading: boolean) => set({ llmLoading: loading }),

    // TTS actions
    loadTts: async (modelId: string) => {
      set({ ttsLoading: true });
      try {
        const result = await commands.loadLocalTts(modelId);
        if (result.status === "ok") {
          set({ ttsLoaded: true });
        } else {
          console.error("Failed to load TTS:", result.error);
        }
      } catch (error) {
        console.error("Failed to load TTS:", error);
      } finally {
        set({ ttsLoading: false });
      }
    },

    unloadTts: async () => {
      try {
        await commands.unloadLocalTts();
        set({ ttsLoaded: false });
      } catch (error) {
        console.error("Failed to unload TTS:", error);
      }
    },

    setTtsLoading: (loading: boolean) => set({ ttsLoading: loading }),

    // Discord actions
    updateDiscordState: (state: Partial<DiscordState>) => {
      set({
        discordConnected: state.connected ?? get().discordConnected,
        discordInVoice: state.in_voice ?? get().discordInVoice,
        discordGuild: state.guild_name ?? get().discordGuild,
        discordChannel: state.channel_name ?? get().discordChannel,
      });
    },

    setDiscordConversationRunning: (running: boolean) => {
      set({ discordConversationRunning: running });
    },

    // Onichan actions
    updateOnichanState: (state: Partial<OnichanState>) => {
      set({
        onichanActive: state.active ?? get().onichanActive,
        onichanMode: state.mode ?? get().onichanMode,
        llmLoaded: state.local_llm_loaded ?? get().llmLoaded,
        ttsLoaded: state.local_tts_loaded ?? get().ttsLoaded,
      });
    },

    setOnichanConversationRunning: (running: boolean) => {
      set({ onichanConversationRunning: running });
    },

    // Memory actions
    updateMemoryState: (state: Partial<MemoryState>) => {
      set({
        memoryRunning: state.is_running ?? get().memoryRunning,
        memoryModelLoaded: state.model_loaded ?? get().memoryModelLoaded,
        memoryCount: state.total_memories ?? get().memoryCount,
      });
    },
  }))
);

// Hook for initializing the store (call once at app startup)
export const initializeSidecarStore = () => {
  const { initialize } = useSidecarStore.getState();
  initialize();
};

// Hook for cleanup (call on app unmount)
export const cleanupSidecarStore = () => {
  const { cleanup } = useSidecarStore.getState();
  cleanup();
};

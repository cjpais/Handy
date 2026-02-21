import {
  MOCK_APP_DIR,
  MOCK_AUDIO_FILE,
  MOCK_HISTORY,
  MOCK_LOG_DIR,
  MOCK_MODELS,
  MOCK_POST_PROCESS_PROVIDERS,
  MOCK_PROMPTS,
  MOCK_RECORDINGS_DIR,
  MOCK_SETTINGS,
} from "./data";

const ok = <T>(data: T) => ({ status: "ok" as const, data });

export const commands = {
  // App + system
  getAppSettings: async () => ok(MOCK_SETTINGS),
  getDefaultSettings: async () => ok(MOCK_SETTINGS),
  getAppDirPath: async () => ok(MOCK_APP_DIR),
  openAppDataDir: async () => ok(null),
  getLogDirPath: async () => ok(MOCK_LOG_DIR),
  openLogDir: async () => ok(null),

  // Permissions + shortcuts
  initializeEnigo: async () => ok(null),
  initializeShortcuts: async () => ok(null),
  suspendBinding: async () => ok(null),
  resumeBinding: async () => ok(null),
  startHandyKeysRecording: async () => ok(null),
  stopHandyKeysRecording: async () => ok(null),
  changeKeyboardImplementationSetting: async () =>
    ok({ success: true, reset_bindings: [] }),

  // Models
  getTranscriptionModelStatus: async () => ok(MOCK_SETTINGS.selected_model),
  isRecording: async () => false,

  // Hardware + environment
  isLaptop: async () => ok(true),

  // Post-processing
  addPostProcessPrompt: async (name: string, prompt: string) =>
    ok({ id: `prompt_${Date.now()}`, name, prompt }),
  updatePostProcessPrompt: async () => ok(null),
  deletePostProcessPrompt: async () => ok(null),
  checkAppleIntelligenceAvailable: async () => false,

  // History
  getHistoryEntries: async () => ok(MOCK_HISTORY),
  toggleHistoryEntrySaved: async () => ok(null),
  getAudioFilePath: async () => ok(MOCK_AUDIO_FILE),
  deleteHistoryEntry: async () => ok(null),
  openRecordingsFolder: async () => ok(MOCK_RECORDINGS_DIR),

  // Misc settings
  setModelUnloadTimeout: async () => ok(null),

  // Models list for storybook previews (not used by components directly)
  getAvailableModels: async () => ok(MOCK_MODELS),

  // Provider data for storybook previews (not used by components directly)
  getPostProcessProviders: async () => ok(MOCK_POST_PROCESS_PROVIDERS),
  getPostProcessPrompts: async () => ok(MOCK_PROMPTS),
};

export type {
  AppSettings,
  AudioDevice,
  ClipboardHandling,
  EngineType,
  HistoryEntry,
  ImplementationChangeResult,
  KeyboardImplementation,
  LLMPrompt,
  LogLevel,
  ModelInfo,
  ModelUnloadTimeout,
  OverlayPosition,
  PasteMethod,
  PostProcessProvider,
  RecordingRetentionPeriod,
  Result,
  ShortcutBinding,
  SoundTheme,
} from "../../../src/bindings";

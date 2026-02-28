import type {
  AppSettings,
  AudioDevice,
  HistoryEntry,
  ModelInfo,
  PostProcessProvider,
  LLMPrompt,
  ShortcutBinding,
} from "../../../src/bindings";

export type Result<T, E> =
  | { status: "ok"; data: T }
  | { status: "error"; error: E };

export const MOCK_BINDINGS: Record<string, ShortcutBinding> = {
  toggle_recording: {
    id: "toggle_recording",
    name: "Toggle Recording",
    description: "Start or stop recording",
    default_binding: "Cmd+Shift+Space",
    current_binding: "Cmd+Shift+Space",
  },
  push_to_talk: {
    id: "push_to_talk",
    name: "Push To Talk",
    description: "Hold to record",
    default_binding: "Fn+Space",
    current_binding: "Fn+Space",
  },
};

export const MOCK_POST_PROCESS_PROVIDERS: PostProcessProvider[] = [
  {
    id: "openai",
    label: "OpenAI",
    base_url: "https://api.openai.com/v1",
    allow_base_url_edit: false,
  },
  {
    id: "custom",
    label: "Custom",
    base_url: "https://llm.example.com/v1",
    allow_base_url_edit: true,
  },
  {
    id: "apple_intelligence",
    label: "Apple Intelligence",
    base_url: "",
    allow_base_url_edit: false,
  },
];

export const MOCK_PROMPTS: LLMPrompt[] = [
  {
    id: "clean_up",
    name: "Clean Up",
    prompt: "Fix punctuation and remove filler words.",
  },
  {
    id: "meeting_notes",
    name: "Meeting Notes",
    prompt: "Convert the transcript into bullet-point meeting notes.",
  },
];

export const MOCK_SETTINGS: AppSettings = {
  bindings: MOCK_BINDINGS,
  push_to_talk: true,
  audio_feedback: true,
  audio_feedback_volume: 0.7,
  sound_theme: "marimba",
  start_hidden: false,
  autostart_enabled: true,
  update_checks_enabled: true,
  selected_model: "whisper-small",
  always_on_microphone: false,
  selected_microphone: "Built-in Microphone",
  clamshell_microphone: "Default",
  selected_output_device: "Default",
  translate_to_english: false,
  selected_language: "en",
  overlay_position: "bottom",
  debug_mode: true,
  log_level: "info",
  custom_words: ["Handy", "Tauri", "Whisper"],
  model_unload_timeout: "never",
  word_correction_threshold: 0.6,
  history_limit: 20,
  recording_retention_period: "weeks2",
  paste_method: "ctrl_v",
  clipboard_handling: "copy_to_clipboard",
  post_process_enabled: true,
  post_process_provider_id: "openai",
  post_process_providers: MOCK_POST_PROCESS_PROVIDERS,
  post_process_api_keys: { openai: "sk-demo-key" },
  post_process_models: { openai: "gpt-4o-mini", custom: "custom-llm" },
  post_process_prompts: MOCK_PROMPTS,
  post_process_selected_prompt_id: "clean_up",
  mute_while_recording: true,
  append_trailing_space: true,
  app_language: "en",
  experimental_enabled: true,
  keyboard_implementation: "tauri",
  paste_delay_ms: 80,
};

export const MOCK_AUDIO_DEVICES: AudioDevice[] = [
  {
    index: "default",
    name: "Default",
    is_default: true,
  },
  {
    index: "mic-1",
    name: "Built-in Microphone",
    is_default: false,
  },
  {
    index: "mic-2",
    name: "USB Podcast Mic",
    is_default: false,
  },
];

export const MOCK_OUTPUT_DEVICES: AudioDevice[] = [
  {
    index: "default",
    name: "Default",
    is_default: true,
  },
  {
    index: "out-1",
    name: "Studio Speakers",
    is_default: false,
  },
  {
    index: "out-2",
    name: "USB Headset",
    is_default: false,
  },
];

export const MOCK_MODELS: ModelInfo[] = [
  {
    id: "whisper-small",
    name: "Whisper Small",
    description: "Good balance of speed and accuracy",
    filename: "ggml-small.bin",
    url: null,
    size_mb: 488,
    is_downloaded: true,
    is_downloading: false,
    partial_size: 0,
    is_directory: false,
    engine_type: "Whisper",
    accuracy_score: 0.68,
    speed_score: 0.72,
    supports_translation: true,
    is_recommended: true,
    supported_languages: ["en", "es", "fr", "de"],
  },
  {
    id: "whisper-medium",
    name: "Whisper Medium",
    description: "Higher accuracy with moderate speed",
    filename: "ggml-medium.bin",
    url: null,
    size_mb: 1530,
    is_downloaded: false,
    is_downloading: false,
    partial_size: 0,
    is_directory: false,
    engine_type: "Whisper",
    accuracy_score: 0.8,
    speed_score: 0.55,
    supports_translation: true,
    is_recommended: false,
    supported_languages: ["en", "es", "fr", "de", "it"],
  },
  {
    id: "whisper-large",
    name: "Whisper Large",
    description: "Maximum accuracy, slower on older machines",
    filename: "ggml-large.bin",
    url: null,
    size_mb: 2950,
    is_downloaded: false,
    is_downloading: true,
    partial_size: 620,
    is_directory: false,
    engine_type: "Whisper",
    accuracy_score: 0.9,
    speed_score: 0.35,
    supports_translation: true,
    is_recommended: false,
    supported_languages: ["en", "es", "fr", "de", "it", "pt"],
  },
];

export const MOCK_DOWNLOAD_PROGRESS = {
  "whisper-large": {
    model_id: "whisper-large",
    downloaded: 620,
    total: 2950,
    percentage: 21,
  },
};

export const MOCK_DOWNLOAD_STATS = {
  "whisper-large": {
    startTime: 0,
    lastUpdate: 0,
    totalDownloaded: 620,
    speed: 12.4,
  },
};

export const MOCK_HISTORY: HistoryEntry[] = [
  {
    id: 1,
    file_name: "meeting-sync.wav",
    timestamp: 1700000000,
    saved: true,
    title: "Weekly sync",
    transcription_text: "We discussed Q1 goals and next steps.",
    post_processed_text:
      "Weekly sync notes: Q1 goals, shipping milestones, and next steps.",
    post_process_prompt: "Meeting Notes",
  },
  {
    id: 2,
    file_name: "voice-memo.wav",
    timestamp: 1700500000,
    saved: false,
    title: "Voice memo",
    transcription_text: "Remember to follow up with design.",
    post_processed_text: null,
    post_process_prompt: null,
  },
];

export const MOCK_APP_DIR = "/Users/edward/Library/Application Support/Handy";
export const MOCK_LOG_DIR = "/Users/edward/Library/Logs/Handy";
export const MOCK_RECORDINGS_DIR =
  "/Users/edward/Library/Application Support/Handy/recordings";
export const MOCK_AUDIO_FILE =
  "/Users/edward/Library/Application Support/Handy/recordings/meeting-sync.wav";

import type { DailyWords, ModelInfo, VoxaStats } from "@/bindings";

/** The preview pins "today" so screenshots do not change from day to day. */
export const PREVIEW_TODAY = "2026-10-17";

const dayBefore = (isoDay: string, back: number): string => {
  const [y, m, d] = isoDay.split("-").map(Number);
  const date = new Date(Date.UTC(y, m - 1, d - back));
  return date.toISOString().slice(0, 10);
};

/** Deterministic words per day: busy weekdays, quiet weekends, a few gaps. */
const WORDS_BY_AGE = [
  412, 388, 64, 0, 301, 522, 447, 365, 120, 0, 289, 334, 410, 276, 198, 0, 22,
  356, 470, 501, 318, 87, 0, 244, 391, 262, 305, 143, 0, 96,
];

export const statsFixture = (): VoxaStats => {
  const daily: DailyWords[] = WORDS_BY_AGE.map((words, age) => ({
    date: dayBefore(PREVIEW_TODAY, age),
    words,
    dictations: words === 0 ? 0 : Math.max(1, Math.round(words / 34)),
  })).reverse();

  return {
    today: { dictations: 12, words: 412, minutes: 2.9 },
    week: { dictations: 61, words: 2134, minutes: 14.8 },
    all_time: { dictations: 188, words: 6420, minutes: 46.3 },
    avg_words_per_dictation: 34.1,
    avg_seconds_per_dictation: 14.8,
    latin_share: 0.27,
    daily,
    models: [
      {
        model_id:
          "handy-computer/whisper-large-v3-turbo-gguf/whisper-large-v3-turbo-Q8_0.gguf",
        dictations: 164,
        words: 5610,
        latin_share: 0.29,
        latency_samples: 140,
        avg_latency_ms: 1180,
        p95_latency_ms: 2310,
        enough_latency_data: true,
      },
      {
        model_id:
          "handy-computer/gigaam-v3-e2e-ctc-gguf/gigaam-v3-e2e-ctc-Q8_0.gguf",
        dictations: 18,
        words: 410,
        latin_share: 0.04,
        latency_samples: 12,
        avg_latency_ms: 420,
        p95_latency_ms: 700,
        enough_latency_data: false,
      },
      {
        model_id: "",
        dictations: 6,
        words: 400,
        latin_share: 0.18,
        latency_samples: 0,
        avg_latency_ms: null,
        p95_latency_ms: null,
        enough_latency_data: false,
      },
    ],
    candidates: [
      {
        kind: "replacement",
        term: "Obsidian",
        count: 7,
        variants: ["Obsidan", "Obsedian"],
        in_dictionary: true,
      },
      {
        kind: "replacement",
        term: "LangChain",
        count: 5,
        variants: ["langchain", "Lang Chain"],
        in_dictionary: true,
      },
      {
        kind: "replacement",
        term: "Kubernetes",
        count: 3,
        variants: ["Kubernates"],
        in_dictionary: false,
      },
      {
        kind: "translit",
        term: "Python",
        count: 9,
        variants: ["питон", "питоне", "питоном"],
        in_dictionary: true,
      },
      {
        kind: "translit",
        term: "TypeScript",
        count: 4,
        variants: ["тайпскрипт", "тайпскрипте"],
        in_dictionary: false,
      },
      {
        kind: "translit",
        term: "Redis",
        count: 2,
        variants: ["редис"],
        in_dictionary: false,
      },
    ],
  };
};

const model = (
  id: string,
  name: string,
  description: string,
  sizeMb: number,
  downloaded: boolean,
  extra: Partial<ModelInfo> = {},
): ModelInfo => ({
  id,
  name,
  description,
  filename: id.split("/").pop() ?? id,
  source: {
    HuggingFace: {
      repo_id: id.split("/").slice(0, 2).join("/"),
      revision: "main",
    },
  },
  size_mb: sizeMb,
  is_downloaded: downloaded,
  is_downloading: false,
  partial_size: 0,
  accuracy_score: 0.88,
  speed_score: 0.35,
  supports_translation: false,
  is_recommended: false,
  supported_languages: ["ru", "en"],
  supports_language_selection: true,
  is_custom: false,
  supports_streaming: false,
  supports_language_detection: true,
  ...extra,
});

export const modelsFixture = (): ModelInfo[] => [
  model(
    "handy-computer/whisper-large-v3-turbo-gguf/whisper-large-v3-turbo-Q8_0.gguf",
    "Whisper Large v3 Turbo",
    "Best pick for Russian with English tech terms. The Q8_0 build (0.9 GB) is recommended for Macs with 8 GB of RAM.",
    845,
    true,
    { is_recommended: true },
  ),
  model(
    "handy-computer/gigaam-v3-e2e-ctc-gguf/gigaam-v3-e2e-ctc-Q8_0.gguf",
    "GigaAM v3 E2E-CTC",
    "Russian speech-to-text with token-level timestamps.",
    259,
    true,
    {
      accuracy_score: 0.69,
      speed_score: 0.98,
      supported_languages: ["ru"],
      supports_language_detection: false,
    },
  ),
  model(
    "handy-computer/parakeet-unified-en-0.6b-gguf/parakeet-unified-en-0.6b-Q8_0.gguf",
    "Parakeet Unified EN 0.6B",
    "Fast, accurate live English transcription",
    698,
    false,
    {
      is_recommended: true,
      supported_languages: ["en"],
      supports_streaming: true,
      accuracy_score: 0.9,
      speed_score: 0.79,
    },
  ),
];

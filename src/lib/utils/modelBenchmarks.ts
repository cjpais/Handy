import type { ModelBenchmarks } from "@/bindings";
import { getLanguageLabel, recognitionLanguage } from "../constants/languages";

// Speed fill mirrors scripts/gen_catalog.py: speed = 1 − e^(−RTF/8), a
// saturating curve since "×" has no ceiling. Recomputed here from the number
// that is shown, so a per-language figure moves the bar with it.
const SPEED_SCALE = 8;

// Quant preference when the model's own quant has no measurement, same order
// as `headline_wer` in gen_catalog.py.
const QUANT_FALLBACK = [
  "q8_0",
  "f16",
  "q5_k_m",
  "q6_k",
  "q4_k_m",
  "f32",
  "bf16",
];

// Eval-set preference when nothing language-specific applies, same order as
// `pick_wer` in gen_catalog.py (LibriSpeech is the common English yardstick).
// Both are English, so they count as a language match for `en`.
const HEADLINE_SETS = ["librispeech_test_clean", "fleurs_en"];

// Vendor eval sets, most comparable first: FLEURS is what Handy's own
// per-language runs use, so a FLEURS number sits next to a measured FLEURS
// number on another card without an apples-to-oranges jump.
const REPORTED_SETS = [
  "fleurs",
  "commonvoice",
  "commonvoice15",
  "covost",
  "mls",
];

// The reference machine the catalog's speed score is derived from, and the
// backend order gen_catalog.py uses on it. Only this machine is shown: an RTF
// from a different box (the cards also carry Apple M4 Max numbers) is not
// comparable across cards, so a model measured only elsewhere shows no number
// and keeps the catalog's floor score.
const REFERENCE_MACHINE = "ryzen_4750u";
const BACKEND_ORDER = ["vulkan", "cpu"];

/** Where a WER number comes from. */
export type WerSource = "measured" | "reported";

export interface WerMeasurement {
  /** Error rate in percent, lower is better. */
  value: number;
  /** Unit used by the benchmark, preserved in the card's label and tooltip. */
  metric: "wer" | "cer";
  /** `measured`: Handy's run on the shipped GGUF; `reported`: the vendor's number for the base model. */
  source: WerSource;
  /** Vendor name for `reported` ("NVIDIA", "OpenAI", …). */
  sourceName?: string;
  /** Eval set key: `fleurs_ru` / `librispeech_test_clean` (measured) or `fleurs` / `commonvoice15` (reported). */
  dataset: string;
  /** Language code the number is for, when the set is per-language. */
  language?: string;
  /** Quant the number was measured at (`measured` only). */
  quant?: string;
  /** Whether the number is for the language the user asked about. */
  matchesLanguage: boolean;
}

/**
 * What the card shows for accuracy given the user's language context.
 * `wer` is the number to display (null when nothing exists at all);
 * `fallback` is the headline (usually English) number kept as context when
 * the requested language has no data (`wer` is null then, `language` set).
 */
export interface WerDisplay {
  wer: WerMeasurement | null;
  /** The language that was asked for and has no number (recognition code). */
  missingLanguage: string | null;
  fallback: WerMeasurement | null;
}

export interface RtfMeasurement {
  /** Real-time factor: seconds of audio transcribed per wall-clock second. */
  value: number;
  machine: string;
  backend: string;
}

type Leaf = Partial<{ [key in string]: number }>;

const pickQuant = (
  byQuant: Leaf | undefined,
  preferredQuant: string | null,
): { quant: string; value: number } | null => {
  if (!byQuant) return null;
  const order = preferredQuant
    ? [preferredQuant.toLowerCase(), ...QUANT_FALLBACK]
    : QUANT_FALLBACK;
  for (const q of order) {
    const v = byQuant[q];
    if (typeof v === "number") return { quant: q, value: v };
  }
  const first = Object.entries(byQuant).find(([, v]) => typeof v === "number");
  return first ? { quant: first[0], value: first[1] as number } : null;
};

// `fleurs_ru` → "ru", `fleurs_ar_test` → "ar"; anything else → undefined.
export const fleursLanguage = (dataset: string): string | undefined => {
  const m = dataset.match(/^fleurs_([a-z]{2,3})(?:_|$)/);
  return m ? m[1] : undefined;
};

/** The language a card should show numbers for: an explicit code, or null for "no preference". */
export const languageContext = (
  ...candidates: (string | undefined | null)[]
): string | null => {
  for (const c of candidates) {
    if (c && c !== "all" && c !== "auto") return recognitionLanguage(c);
  }
  return null;
};

const measuredFor = (
  benchmarks: ModelBenchmarks,
  dataset: string,
  preferredQuant: string | null,
  matchesLanguage: boolean,
): WerMeasurement | null => {
  const picked = pickQuant(benchmarks.wer[dataset], preferredQuant);
  return picked
    ? {
        value: picked.value,
        metric: "wer",
        source: "measured",
        dataset,
        language: fleursLanguage(dataset),
        quant: picked.quant,
        matchesLanguage,
      }
    : null;
};

/** Handy's headline measurement: LibriSpeech, then FLEURS-en, then whatever single set the card has. */
const headlineMeasured = (
  benchmarks: ModelBenchmarks,
  preferredQuant: string | null,
  wanted: string | null,
): WerMeasurement | null => {
  const sets = Object.keys(benchmarks.wer);
  for (const s of HEADLINE_SETS) {
    if (sets.includes(s)) {
      const m = measuredFor(benchmarks, s, preferredQuant, wanted === "en");
      if (m) return m;
    }
  }
  for (const s of sets) {
    const m = measuredFor(
      benchmarks,
      s,
      preferredQuant,
      fleursLanguage(s) === wanted,
    );
    if (m) return m;
  }
  return null;
};

const reportedFor = (
  benchmarks: ModelBenchmarks,
  lang: string,
): WerMeasurement | null => {
  const rep = benchmarks.reported;
  if (!rep) return null;
  const availableSets = [
    ...new Set([...Object.keys(rep.wer), ...Object.keys(rep.cer ?? {})]),
  ];
  const sets = [
    ...REPORTED_SETS.filter((s) => availableSets.includes(s)),
    ...availableSets.filter((s) => !REPORTED_SETS.includes(s)),
  ];
  for (const dataset of sets) {
    for (const metric of ["wer", "cer"] as const) {
      const v = rep[metric]?.[dataset]?.[lang];
      if (typeof v === "number") {
        return {
          value: v,
          metric,
          source: "reported",
          sourceName: rep.source,
          dataset,
          language: lang,
          matchesLanguage: true,
        };
      }
    }
  }
  return null;
};

/**
 * Resolve the accuracy number for a card.
 *
 * With a language in context: Handy's own FLEURS run for that language, else
 * the vendor's published number for it, else nothing for that language (the
 * headline English number is returned separately as `fallback`, so the card
 * can say "no <lang> data · EN 1.9%" instead of passing an English number off
 * as the answer). Without a language: the headline measurement.
 */
export const resolveWer = (
  benchmarks: ModelBenchmarks | null | undefined,
  language: string | null,
  preferredQuant: string | null,
): WerDisplay => {
  const none: WerDisplay = { wer: null, missingLanguage: null, fallback: null };
  if (!benchmarks) return none;
  const headline = headlineMeasured(benchmarks, preferredQuant, language);
  if (!language)
    return { wer: headline, missingLanguage: null, fallback: null };

  const ownSet = Object.keys(benchmarks.wer).find(
    (s) => fleursLanguage(s) === language,
  );
  if (ownSet) {
    const m = measuredFor(benchmarks, ownSet, preferredQuant, true);
    if (m) return { wer: m, missingLanguage: null, fallback: null };
  }
  if (headline?.matchesLanguage) {
    return { wer: headline, missingLanguage: null, fallback: null };
  }
  const rep = reportedFor(benchmarks, language);
  if (rep) return { wer: rep, missingLanguage: null, fallback: headline };
  if (!headline && !benchmarks.reported) return none;
  return { wer: null, missingLanguage: language, fallback: headline };
};

/** Speed on the catalog's reference machine, best available backend. */
export const pickRtf = (
  benchmarks: ModelBenchmarks | null | undefined,
): RtfMeasurement | null => {
  const byBackend = benchmarks?.rtf?.[REFERENCE_MACHINE];
  if (!byBackend) return null;
  for (const backend of BACKEND_ORDER) {
    const v = byBackend[backend];
    if (typeof v === "number" && v > 0) {
      return { value: v, machine: REFERENCE_MACHINE, backend };
    }
  }
  return null;
};

/**
 * 0–1 bar fill for a WER: word accuracy, the same quantity the label beside
 * the bar prints. A bar next to a percentage is read as that percentage, so
 * the two must agree; the catalog's e^(−WER/15) spread is kept only as the
 * fallback fill for entries without card data.
 */
export const accuracyFromWer = (wer: number): number =>
  Math.max(0, Math.min(1, 1 - wer / 100));

/** 0–1 bar fill for an RTF, identical to the catalog's `speed_score` formula. */
export const speedFromRtf = (rtf: number): number =>
  1 - Math.exp(-rtf / SPEED_SCALE);

/** "1.9%" / "5.4%" / "12%" / "105%" — one decimal below 10, none above. */
export const formatWer = (wer: number): string =>
  `${wer < 10 ? wer.toFixed(1) : Math.round(wer).toString()}%`;

/** Word accuracy for the card: 100 − WER, floored at 0 — "94.5%", "69%", "0%". */
export const formatAccuracy = (wer: number): string => {
  const acc = Math.max(0, 100 - wer);
  return `${acc >= 90 ? acc.toFixed(1) : Math.round(acc).toString()}%`;
};

/** "7.5×" / "12×" / "163×". */
export const formatRtf = (rtf: number): string =>
  `${rtf < 10 ? rtf.toFixed(1) : Math.round(rtf).toString()}×`;

const DATASET_LABELS: Record<string, string> = {
  librispeech_test_clean: "LibriSpeech test-clean",
  fleurs: "FLEURS",
  commonvoice: "Common Voice",
  commonvoice15: "Common Voice",
  covost: "CoVoST",
  mls: "MLS",
};

export const datasetLabel = (wer: WerMeasurement): string => {
  const base = wer.dataset.startsWith("fleurs_")
    ? "FLEURS"
    : (DATASET_LABELS[wer.dataset] ?? wer.dataset);
  const lang =
    wer.language ??
    (wer.dataset === "librispeech_test_clean" ? "en" : undefined);
  return lang ? `${base} (${getLanguageLabel(lang) || lang})` : base;
};

// Human names for the reference machines that appear in the catalog.
const MACHINE_LABELS: Record<string, string> = {
  ryzen_4750u: "Ryzen 7 4750U",
  m4_max: "Apple M4 Max",
};

export const machineLabel = (machine: string): string =>
  MACHINE_LABELS[machine] ?? machine;

const BACKEND_LABELS: Record<string, string> = {
  cpu: "CPU",
  vulkan: "Vulkan",
  metal: "Metal",
  cuda: "CUDA",
};

export const backendLabel = (backend: string): string =>
  BACKEND_LABELS[backend] ?? backend;

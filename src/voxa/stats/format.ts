import type { DailyWords } from "@/bindings";

/** Latency numbers need this many timed dictations per model (mirrors the backend). */
export const MIN_LATENCY_SAMPLES = 20;

/** `1234567` -> `1 234 567` (locale aware). */
export function formatNumber(value: number, locale: string): string {
  return new Intl.NumberFormat(locale).format(value);
}

/** Share `0.4231` -> `42%`; `null` -> an em dash. */
export function formatPercent(share: number | null | undefined): string {
  if (share === null || share === undefined || Number.isNaN(share)) return "—";
  return `${Math.round(share * 100)}%`;
}

/** Milliseconds as `850 ms` / `1.2 s` units picked by the caller's labels. */
export function splitDuration(ms: number): { value: number; unit: "ms" | "s" } {
  if (ms < 1000) return { value: Math.round(ms), unit: "ms" };
  return { value: Math.round(ms / 100) / 10, unit: "s" };
}

/** Minutes of audio -> minutes below two hours, hours above. */
export function splitMinutes(minutes: number): {
  value: number;
  unit: "min" | "h";
} {
  if (minutes < 120)
    return { value: Math.round(minutes * 10) / 10, unit: "min" };
  return { value: Math.round((minutes / 60) * 10) / 10, unit: "h" };
}

export interface BarGeometry {
  date: string;
  words: number;
  dictations: number;
  /** Bar height as a share of the chart height, 0..1 (0 stays 0). */
  ratio: number;
  isToday: boolean;
}

/**
 * Bar heights for the words-per-day chart. The scale tops out at the busiest
 * day, so a quiet month still fills the chart. A day with words never rounds to
 * an invisible bar: it gets at least `minRatio`.
 */
export function buildBars(
  daily: DailyWords[],
  minRatio = 0.04,
): { bars: BarGeometry[]; max: number } {
  const max = daily.reduce((m, d) => Math.max(m, d.words), 0);
  const bars = daily.map((d, i) => ({
    date: d.date,
    words: d.words,
    dictations: d.dictations,
    ratio: max === 0 || d.words === 0 ? 0 : Math.max(minRatio, d.words / max),
    isToday: i === daily.length - 1,
  }));
  return { bars, max };
}

/** `2026-03-11` -> `11.03` without going through a timezone-shifting Date. */
export function shortDate(isoDay: string): string {
  const [, month, day] = isoDay.split("-");
  return `${day}.${month}`;
}

/** A model registry id as a short label: the file name without extension. */
export function modelLabel(modelId: string): string {
  const last = modelId.split("/").pop() ?? modelId;
  return last.replace(/\.(gguf|bin)$/i, "");
}

const QUANT_SUFFIX = /^(.*?)-((?:Q\d(?:_K(?:_[MSL])?|_\d)?)|F16|F32|BF16)$/i;

/** `…/whisper-large-v3-turbo-Q8_0.gguf` -> name `whisper-large-v3-turbo`, quant `Q8_0`. */
export function splitModelId(modelId: string): {
  name: string;
  quant: string | null;
} {
  const label = modelLabel(modelId);
  const match = QUANT_SUFFIX.exec(label);
  return match
    ? { name: match[1], quant: match[2] }
    : { name: label, quant: null };
}

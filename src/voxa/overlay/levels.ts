/** Microphone level helpers for the glass capsule's waveform. */

/** Bars in the capsule's waveform. */
export const BAR_COUNT = 7;
/** Shortest and tallest bar, in px. */
export const BAR_MIN_PX = 4;
export const BAR_MAX_PX = 26;
/** The centre bars move most, like a voice envelope. */
const SHAPE = [0.62, 0.8, 0.94, 1, 0.94, 0.8, 0.62];

const clamp01 = (v: number) =>
  Number.isFinite(v) ? Math.min(1, Math.max(0, v)) : 0;

/**
 * One exponential smoothing step over the level buckets:
 * `alpha` is the weight of the new reading (0.3 = calm, 1 = no smoothing).
 */
export function smoothLevels(
  prev: number[],
  next: number[],
  alpha = 0.3,
): number[] {
  const length = Math.max(prev.length, next.length);
  return Array.from({ length }, (_, i) => {
    const before = clamp01(prev[i] ?? 0);
    const target = clamp01(next[i] ?? 0);
    return before * (1 - alpha) + target * alpha;
  });
}

/** Root mean square of the buckets: the overall loudness, 0..1. */
export function rms(levels: number[]): number {
  if (levels.length === 0) return 0;
  const sum = levels.reduce((acc, v) => acc + clamp01(v) ** 2, 0);
  return Math.sqrt(sum / levels.length);
}

/** Spectrum slices the bars read: 0 = lows (centre bars) .. last = highs (outer bars). */
const GROUPS = 4;

/**
 * Bar levels (0..1) from the smoothed buckets. The bars mirror around the
 * centre like a voice envelope: the centre bar follows the low end of the
 * spectrum, where speech carries most energy, and the outer bars follow higher
 * slices. Every bar also takes part of the overall loudness, so they all move
 * when someone speaks.
 */
export function barLevels(smoothed: number[], count = BAR_COUNT): number[] {
  const overall = rms(smoothed);
  const centre = (count - 1) / 2;
  return Array.from({ length: count }, (_, i) => {
    const group = Math.min(Math.round(Math.abs(i - centre)), GROUPS - 1);
    const start = Math.floor((group * smoothed.length) / GROUPS);
    const end = Math.max(
      start + 1,
      Math.floor(((group + 1) * smoothed.length) / GROUPS),
    );
    const slice = smoothed.slice(start, end);
    const peak = slice.length ? Math.max(...slice.map(clamp01)) : 0;
    const blended = 0.55 * peak + 0.45 * overall;
    const shape = SHAPE[i] ?? 1;
    return clamp01(blended ** 0.7 * 1.3 * shape);
  });
}

/** A bar level as a pixel height between the minimum and maximum bar. */
export function barHeightPx(
  level: number,
  min = BAR_MIN_PX,
  max = BAR_MAX_PX,
): number {
  return Math.round(min + clamp01(level) * (max - min));
}

/** `7` -> `0:07`, `75` -> `1:15`. */
export function formatElapsed(totalSeconds: number): string {
  const seconds = Math.max(0, Math.floor(totalSeconds));
  const minutes = Math.floor(seconds / 60);
  return `${minutes}:${String(seconds % 60).padStart(2, "0")}`;
}

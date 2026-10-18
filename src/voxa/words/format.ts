import type { WordsPeriod } from "@/bindings";

export type PeriodPreset = "week" | "month" | "all" | "custom";

/** Local calendar day as `YYYY-MM-DD` (the backend buckets days in the system timezone too). */
export function isoDay(date: Date): string {
  const y = date.getFullYear();
  const m = String(date.getMonth() + 1).padStart(2, "0");
  const d = String(date.getDate()).padStart(2, "0");
  return `${y}-${m}-${d}`;
}

function daysBefore(date: Date, days: number): Date {
  const copy = new Date(date.getFullYear(), date.getMonth(), date.getDate());
  copy.setDate(copy.getDate() - days);
  return copy;
}

/** A range with both ends set and in order, or `null`. */
export function normalizeRange(
  from: string,
  to: string,
): { from: string; to: string } | null {
  const valid = /^\d{4}-\d{2}-\d{2}$/;
  if (!valid.test(from) || !valid.test(to)) return null;
  // YYYY-MM-DD sorts as text; a reversed pair is swapped, not rejected
  return from <= to ? { from, to } : { from: to, to: from };
}

/**
 * The period a preset stands for. Week and month are rolling windows that end
 * today (7 and 30 days); "all" has no bounds.
 */
export function periodFor(
  preset: PeriodPreset,
  today: Date,
  custom?: { from: string; to: string } | null,
): WordsPeriod {
  switch (preset) {
    case "week":
      return { from: isoDay(daysBefore(today, 6)), to: isoDay(today) };
    case "month":
      return { from: isoDay(daysBefore(today, 29)), to: isoDay(today) };
    case "custom":
      return custom
        ? { from: custom.from, to: custom.to }
        : { from: null, to: null };
    default:
      return { from: null, to: null };
  }
}

/** Uses per 100 words; 0 when there are no words. */
export function per100(count: number, words: number): number {
  return words > 0 ? (count * 100) / words : 0;
}

/** `4.236` -> `4,2` (locale aware). */
export function formatDecimal(
  value: number,
  locale: string,
  digits = 1,
): string {
  return new Intl.NumberFormat(locale, {
    minimumFractionDigits: digits,
    maximumFractionDigits: digits,
  }).format(value);
}

/** `2026-10-12` -> `12 окт.` */
export function weekLabel(day: string, locale: string): string {
  const [y, m, d] = day.split("-").map(Number);
  return new Intl.DateTimeFormat(locale, {
    day: "numeric",
    month: "short",
  }).format(new Date(y, m - 1, d));
}

/** Bar length of a list row, 0..1; a non-zero count never rounds to nothing. */
export function barRatio(count: number, max: number, minRatio = 0.03): number {
  if (max <= 0 || count <= 0) return 0;
  return Math.max(minRatio, count / max);
}

export interface TrendPoint {
  x: number;
  y: number;
  value: number;
}

/**
 * Points of a line chart in a `width` x `height` box with `pad` around it. The
 * scale runs from 0 to the highest value (a little headroom), so a flat zero line
 * sits on the bottom edge.
 */
export function trendPoints(
  values: number[],
  width: number,
  height: number,
  pad = 6,
): { points: TrendPoint[]; max: number } {
  const max = values.reduce((m, v) => Math.max(m, v), 0);
  const top = max === 0 ? 1 : max * 1.15;
  const innerW = width - pad * 2;
  const innerH = height - pad * 2;
  const step = values.length > 1 ? innerW / (values.length - 1) : 0;
  const points = values.map((value, i) => ({
    x: values.length > 1 ? pad + step * i : width / 2,
    y: pad + innerH - (value / top) * innerH,
    value,
  }));
  return { points, max };
}

export function polylinePath(points: TrendPoint[]): string {
  return points
    .map((p, i) => `${i === 0 ? "M" : "L"}${p.x.toFixed(1)} ${p.y.toFixed(1)}`)
    .join(" ");
}

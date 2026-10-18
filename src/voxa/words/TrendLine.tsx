import React, { useState } from "react";
import { useTranslation } from "react-i18next";
import type { WeekPoint } from "@/bindings";
import {
  formatDecimal,
  per100,
  polylinePath,
  trendPoints,
  weekLabel,
} from "./format";

const WIDTH = 520;
const HEIGHT = 130;

/**
 * Fillers per 100 words, week by week, as a line. Hover or focus a point for
 * the week's numbers; the same text is the accessible name of the point.
 */
export const TrendLine: React.FC<{ weeks: WeekPoint[]; locale: string }> = ({
  weeks,
  locale,
}) => {
  const { t } = useTranslation();
  const [active, setActive] = useState<number | null>(null);
  const values = weeks.map((w) => per100(w.fillers, w.words));
  const { points, max } = trendPoints(values, WIDTH, HEIGHT, 10);
  const hasData = weeks.some((w) => w.fillers > 0);
  const describe = (i: number) =>
    t("voxa.words.fillers.week", {
      date: weekLabel(weeks[i].week_start, locale),
      value: formatDecimal(values[i], locale),
      count: weeks[i].fillers,
      words: weeks[i].words,
    });
  const shown = active ?? weeks.length - 1;

  if (weeks.length < 2 || !hasData) {
    return (
      <p className="vg-muted vgw-trend-empty">
        {t("voxa.words.fillers.trendEmpty")}
      </p>
    );
  }

  return (
    <figure className="vgw-trend" data-testid="filler-trend">
      <figcaption className="vgw-trend-head">
        <span className="vg-eyebrow">{t("voxa.words.fillers.trendTitle")}</span>
        <span className="vgw-trend-readout" aria-live="polite">
          {describe(shown)}
        </span>
      </figcaption>
      <svg
        viewBox={`0 0 ${WIDTH} ${HEIGHT}`}
        className="vgw-trend-svg"
        role="group"
        aria-label={t("voxa.words.fillers.trendTitle")}
        onMouseLeave={() => setActive(null)}
      >
        <line
          x1="10"
          y1={HEIGHT - 10}
          x2={WIDTH - 10}
          y2={HEIGHT - 10}
          className="vgw-trend-base"
        />
        <path d={polylinePath(points)} className="vgw-trend-line" />
        {points.map((p, i) => (
          <circle
            key={weeks[i].week_start}
            cx={p.x}
            cy={p.y}
            r={active === i ? 5.5 : 3.5}
            className={`vgw-trend-dot${i === weeks.length - 1 ? " is-now" : ""}`}
            tabIndex={0}
            role="img"
            aria-label={describe(i)}
            onMouseEnter={() => setActive(i)}
            onFocus={() => setActive(i)}
            onBlur={() => setActive(null)}
          />
        ))}
      </svg>
      <div className="vgw-trend-axis">
        <span>{weekLabel(weeks[0].week_start, locale)}</span>
        <span className="vg-muted">
          {t("voxa.words.fillers.peak", { value: formatDecimal(max, locale) })}
        </span>
        <span>{weekLabel(weeks[weeks.length - 1].week_start, locale)}</span>
      </div>
    </figure>
  );
};

/** A tiny line of one filler's weekly counts, for its row. */
export const Sparkline: React.FC<{ values: number[] }> = ({ values }) => {
  const { points } = trendPoints(values, 72, 20, 2);
  const flat = values.every((v) => v === 0);
  return (
    <svg
      viewBox="0 0 72 20"
      className="vgw-spark"
      aria-hidden="true"
      focusable="false"
    >
      <path d={polylinePath(points)} className={flat ? "is-flat" : ""} />
    </svg>
  );
};

import React, { useState } from "react";
import { useTranslation } from "react-i18next";
import type { DailyWords } from "@/bindings";
import { buildBars, formatNumber, shortDate } from "./format";

interface DailyBarsProps {
  daily: DailyWords[];
  locale: string;
}

/** Words per day, one bar per day, newest on the right. Plain CSS bars. */
export const DailyBars: React.FC<DailyBarsProps> = ({ daily, locale }) => {
  const { t } = useTranslation();
  const { bars, max } = buildBars(daily);
  const [hovered, setHovered] = useState<number | null>(null);
  const focus = hovered !== null ? bars[hovered] : null;

  return (
    <div className="vg-chart">
      <div className="vg-chart-head">
        <div>
          <h3 className="vg-card-title">{t("voxa.stats.chart.title")}</h3>
          <p className="vg-muted">{t("voxa.stats.chart.range")}</p>
        </div>
        <div className="vg-chart-readout" aria-live="polite">
          {focus ? (
            <>
              <span className="vg-chart-readout-date">
                {shortDate(focus.date)}
              </span>
              <strong>{formatNumber(focus.words, locale)}</strong>
            </>
          ) : (
            <span className="vg-muted">
              {t("voxa.stats.chart.peak", {
                value: formatNumber(max, locale),
              })}
            </span>
          )}
        </div>
      </div>

      <div
        className="vg-bars"
        role="img"
        aria-label={t("voxa.stats.chart.title")}
        onMouseLeave={() => setHovered(null)}
      >
        {bars.map((bar, index) => (
          <div
            key={bar.date}
            className="vg-bar-slot"
            title={t("voxa.stats.chart.bar", {
              date: shortDate(bar.date),
              words: formatNumber(bar.words, locale),
              dictations: bar.dictations,
            })}
            onMouseEnter={() => setHovered(index)}
          >
            <div
              className={`vg-bar ${bar.isToday ? "is-today" : ""} ${
                bar.ratio === 0 ? "is-empty" : ""
              }`}
              style={{ height: `${Math.max(bar.ratio * 100, 0)}%` }}
            />
          </div>
        ))}
      </div>

      <div className="vg-axis">
        <span>{shortDate(bars[0]?.date ?? "0000-00-00")}</span>
        <span>
          {shortDate(bars[Math.floor(bars.length / 2)]?.date ?? "0000-00-00")}
        </span>
        <span>{t("voxa.stats.chart.today")}</span>
      </div>
    </div>
  );
};

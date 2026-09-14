import React, { useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { commands, events } from "@/bindings";
import { SettingsGroup } from "../../ui/SettingsGroup";

const DAYS = 371; // one full year of days to render

const toDateKey = (date: Date): string => {
  const year = date.getFullYear();
  const month = String(date.getMonth() + 1).padStart(2, "0");
  const day = String(date.getDate()).padStart(2, "0");
  return `${year}-${month}-${day}`;
};

/// Map a day's word count to a 0..4 heat level, GitHub-contribution style.
const heatLevel = (words: number, max: number): number => {
  if (words <= 0) return 0;
  if (max <= 4) return Math.min(words, 4);
  const ratio = words / max;
  if (ratio > 0.75) return 4;
  if (ratio > 0.5) return 3;
  if (ratio > 0.25) return 2;
  return 1;
};

const HEAT_COLORS = [
  "color-mix(in srgb, var(--color-text), transparent 90%)",
  "var(--color-heat-1)",
  "var(--color-heat-2)",
  "var(--color-heat-3)",
  "var(--color-heat-4)",
];

const FUTURE_COLOR = "color-mix(in srgb, var(--color-text), transparent 88%)";

interface HeatDay {
  date: Date;
  dateKey: string;
  words: number;
  level: number;
  isFuture: boolean;
}

interface TooltipState {
  date: Date;
  words: number;
  isFuture: boolean;
  x: number;
  y: number;
}

export const InsightsSettings: React.FC = () => {
  const { t, i18n } = useTranslation();
  const [dailyMap, setDailyMap] = useState<Record<string, number>>({});
  const [todayWords, setTodayWords] = useState(0);
  const [totalWords, setTotalWords] = useState(0);
  const [hovered, setHovered] = useState<TooltipState | null>(null);
  const chartRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const load = async () => {
      const [dailyResult, totalResult] = await Promise.all([
        commands.getDailyWords(DAYS),
        commands.getTotalWords(),
      ]);
      if (dailyResult.status === "ok") {
        const map: Record<string, number> = {};
        for (const entry of dailyResult.data) {
          map[entry.date] = entry.words;
        }
        setDailyMap(map);
      }
      if (totalResult.status === "ok") {
        setTotalWords(totalResult.data);
      }
    };

    load();
    const unlisten = events.wordCountChanged.listen((event) => {
      const { total_words, today_words } = event.payload;
      setTotalWords(total_words);
      setTodayWords(today_words);
      setDailyMap((prev) => ({
        ...prev,
        [toDateKey(new Date())]: today_words,
      }));
    });

    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  const { weeks, monthLabels, last7, last30 } = useMemo(() => {
    const today = new Date();
    today.setHours(0, 0, 0, 0);
    const todayKey = toDateKey(today);

    // Extend one Sunday-to-Saturday week past today so the grid has anchor
    // cells that will fill in green on future days.
    const padToWeekEnd = 6 - today.getDay();
    const totalSlots = DAYS + padToWeekEnd;

    const days: HeatDay[] = [];
    for (let i = 0; i < totalSlots; i++) {
      const date = new Date(today);
      date.setDate(today.getDate() - (DAYS - 1) + i);
      const dateKey = toDateKey(date);
      days.push({
        date,
        dateKey,
        words: dailyMap[dateKey] ?? 0,
        level: 0,
        isFuture: dateKey > todayKey,
      });
    }

    const max = days.reduce((acc, d) => Math.max(acc, d.words), 0);
    for (const day of days) {
      if (!day.isFuture) {
        day.level = heatLevel(day.words, max);
      }
    }

    const weeks: HeatDay[][] = [];
    for (let i = 0; i < days.length; i += 7) {
      weeks.push(days.slice(i, i + 7));
    }

    const monthLabels: { index: number; label: string }[] = [];
    const monthFmt = new Intl.DateTimeFormat(i18n.language, { month: "short" });
    let previousMonth = -1;
    weeks.forEach((week, index) => {
      const monthKey = week[0].date.getFullYear() * 12 + week[0].date.getMonth();
      if (monthKey !== previousMonth) {
        previousMonth = monthKey;
        monthLabels.push({ index, label: monthFmt.format(week[0].date) });
      }
    });

    let last7 = 0;
    let last30 = 0;
    for (let i = 0; i < 30; i++) {
      const date = new Date(today);
      date.setDate(today.getDate() - i);
      const words = dailyMap[toDateKey(date)] ?? 0;
      if (i < 7) {
        last7 += words;
      }
      last30 += words;
    }

    return { weeks, monthLabels, last7, last30 };
  }, [dailyMap, i18n.language]);

  const weekdayFmt = new Intl.DateTimeFormat(i18n.language, { weekday: "narrow" });
  const weekdayLabels = [0, 1, 2, 3, 4, 5, 6].map((offset) => {
    const date = new Date(2026, 0, 4 + offset); // Sunday..Saturday
    return weekdayFmt.format(date);
  });

  const dateFmt = new Intl.DateTimeFormat(i18n.language, {
    weekday: "long",
    month: "long",
    day: "numeric",
    year: "numeric",
  });
  const shortDateFmt = new Intl.DateTimeFormat(i18n.language, {
    weekday: "short",
    month: "short",
    day: "numeric",
  });

  const handleCellEnter = (
    day: HeatDay,
    event: React.MouseEvent<HTMLDivElement>,
  ) => {
    const host = chartRef.current;
    if (!host) return;
    const rect = event.currentTarget.getBoundingClientRect();
    const hostRect = host.getBoundingClientRect();
    setHovered({
      date: day.date,
      words: day.words,
      isFuture: day.isFuture,
      x: rect.left - hostRect.left + rect.width / 2,
      y: rect.top - hostRect.top,
    });
  };

  return (
    <div className="max-w-3xl w-full mx-auto space-y-6">
      <SettingsGroup title={t("settings.insights.title")}>
        <div className="grid grid-cols-2 gap-4 p-1">
          <div className="flex flex-col items-center gap-1 p-4 rounded-lg border border-mid-gray/20 bg-text/5">
            <span className="text-3xl font-semibold">
              {todayWords.toLocaleString(i18n.language)}
            </span>
            <span className="text-xs text-text/60 uppercase tracking-wide">
              {t("settings.insights.today")}
            </span>
            <span className="text-[11px] text-text/40">
              {shortDateFmt.format(new Date())}
            </span>
          </div>
          <div className="flex flex-col items-center gap-1 p-4 rounded-lg border border-mid-gray/20 bg-text/5">
            <span className="text-3xl font-semibold">
              {totalWords.toLocaleString(i18n.language)}
            </span>
            <span className="text-xs text-text/60 uppercase tracking-wide">
              {t("settings.insights.lifetime")}
            </span>
          </div>
        </div>

        <div ref={chartRef} className="relative p-1">
          <p className="text-sm font-medium mb-2">
            {t("settings.insights.chartTitle")}
          </p>
          <div className="overflow-x-auto pb-2">
            <div className="inline-flex">
              <div className="flex flex-col justify-between mr-1 py-[2px]">
                {weekdayLabels.map((label, i) => (
                  <span
                    key={i}
                    className="h-3 text-[9px] leading-3 text-text/40"
                  >
                    {label}
                  </span>
                ))}
              </div>
              <div className="flex flex-col gap-1">
                <div
                  className="grid gap-[3px]"
                  style={{ gridTemplateColumns: `repeat(${weeks.length}, 12px)` }}
                >
                  {monthLabels.map(({ index, label }) => (
                    <span
                      key={index}
                      className="text-[9px] leading-3 text-text/40 truncate"
                      style={{ gridColumnStart: index + 1 }}
                    >
                      {label}
                    </span>
                  ))}
                </div>
                <div className="flex gap-[3px]">
                  {weeks.map((week, weekIndex) => (
                    <div key={weekIndex} className="flex flex-col gap-[3px]">
                      {week.map((day) => (
                        <div
                          key={day.dateKey}
                          className="w-3 h-3 rounded-[3px] cursor-pointer"
                          style={{
                            backgroundColor: day.isFuture
                              ? FUTURE_COLOR
                              : HEAT_COLORS[day.level],
                            ...(day.isFuture
                              ? {
                                  outline:
                                    "1px solid color-mix(in srgb, var(--color-mid-gray), transparent 70%)",
                                }
                              : {}),
                          }}
                          onMouseEnter={(e) => handleCellEnter(day, e)}
                          onMouseLeave={() => setHovered(null)}
                        />
                      ))}
                    </div>
                  ))}
                </div>
              </div>
            </div>
          </div>

          <div className="flex items-center justify-end gap-3 text-[11px] text-text/50">
            <span>
              {t("settings.insights.last7")}:{" "}
              <span className="font-medium">
                {last7.toLocaleString(i18n.language)}
              </span>
            </span>
            <span>
              {t("settings.insights.last30")}:{" "}
              <span className="font-medium">
                {last30.toLocaleString(i18n.language)}
              </span>
            </span>
          </div>

          <div className="flex items-center justify-end gap-1 mt-2">
            <span className="text-[10px] text-text/40">
              {t("settings.insights.less")}
            </span>
            {[0, 1, 2, 3, 4].map((level) => (
              <div
                key={level}
                className="w-3 h-3 rounded-[3px]"
                style={{ backgroundColor: HEAT_COLORS[level] }}
              />
            ))}
            <span className="text-[10px] text-text/40">
              {t("settings.insights.more")}
            </span>
          </div>

          {hovered && (
            <div
              className="absolute z-10 px-2 py-1 rounded-md bg-text text-background text-xs font-medium whitespace-nowrap pointer-events-none shadow-lg"
              style={{
                left: hovered.x,
                top: hovered.y - 6,
                transform: "translate(-50%, -100%)",
              }}
            >
              <div>{dateFmt.format(hovered.date)}</div>
              <div>
                {hovered.isFuture
                  ? t("settings.insights.tooltipFuture")
                  : `${hovered.words.toLocaleString(i18n.language)} ${t(
                      "settings.insights.words",
                    )}`}
              </div>
            </div>
          )}
        </div>
      </SettingsGroup>
    </div>
  );
};
import React from "react";
import { useTranslation } from "react-i18next";
import { type AnalyticsStats } from "@/bindings";
import { BarChart3, Clock, Flame, Type } from "lucide-react";
import { StatCard } from "./StatCard";

interface StatsGridProps {
  stats: AnalyticsStats;
}

const formatDuration = (seconds: number): string => {
  if (seconds < 60) return `${Math.round(seconds)}s`;
  if (seconds < 3600) return `${Math.round(seconds / 60)}m`;
  const hours = Math.floor(seconds / 3600);
  const mins = Math.round((seconds % 3600) / 60);
  return `${hours}h ${mins}m`;
};

const formatWpm = (wpm: number | null): string => {
  if (wpm === null) return "-";
  return Math.round(wpm).toString();
};

export const StatsGrid: React.FC<StatsGridProps> = ({ stats }) => {
  const { t } = useTranslation();

  const streakSubtitle =
    stats.current_streak_days === 1
      ? t("settings.analytics.day")
      : t("settings.analytics.days");

  return (
    <div className="px-4 py-4 grid grid-cols-2 gap-4">
      <StatCard
        icon={<Type className="w-5 h-5" />}
        label={t("settings.analytics.totalWords")}
        value={stats.total_words.toLocaleString()}
      />
      <StatCard
        icon={<Clock className="w-5 h-5" />}
        label={t("settings.analytics.recordingTime")}
        value={formatDuration(stats.total_duration_seconds)}
      />
      <StatCard
        icon={<BarChart3 className="w-5 h-5" />}
        label={t("settings.analytics.averageWpm")}
        value={formatWpm(stats.average_wpm)}
        subtitle={t("settings.analytics.wordsPerMinute")}
      />
      <StatCard
        icon={<Flame className="w-5 h-5" />}
        label={t("settings.analytics.currentStreak")}
        value={stats.current_streak_days.toString()}
        subtitle={streakSubtitle}
      />
    </div>
  );
};

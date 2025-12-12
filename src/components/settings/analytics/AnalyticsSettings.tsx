import React, { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { listen } from "@tauri-apps/api/event";
import { type AnalyticsStats, commands } from "@/bindings";
import { type Period, PeriodSelector } from "./PeriodSelector";
import { StatsGrid } from "./StatsGrid";

export const AnalyticsSettings: React.FC = () => {
  const { t } = useTranslation();
  const [period, setPeriod] = useState<Period>("all_time");
  const [stats, setStats] = useState<AnalyticsStats | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const loadAnalytics = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const result = await commands.getAnalytics(period);
      if (result.status === "ok") {
        setStats(result.data);
      } else {
        setError(result.error);
      }
    } catch (err) {
      console.error("Failed to load analytics:", err);
      setError(t("settings.analytics.loadError"));
    } finally {
      setLoading(false);
    }
  }, [period, t]);

  useEffect(() => {
    loadAnalytics();
  }, [loadAnalytics]);

  useEffect(() => {
    const setupListener = async () => {
      return await listen("history-updated", () => {
        loadAnalytics();
      });
    };

    const unlistenPromise = setupListener();

    return () => {
      unlistenPromise.then((unlisten) => {
        if (unlisten) {
          unlisten();
        }
      });
    };
  }, [loadAnalytics]);

  const renderContent = () => {
    if (loading) {
      return (
        <div className="px-4 py-8 text-center text-text/60">
          {t("settings.analytics.loading")}
        </div>
      );
    }

    if (error) {
      return <div className="px-4 py-8 text-center text-red-500">{error}</div>;
    }

    if (stats && stats.transcription_count > 0) {
      return <StatsGrid stats={stats} />;
    }

    return (
      <div className="px-4 py-8 text-center text-text/60">
        {t("settings.analytics.noData")}
      </div>
    );
  };

  return (
    <div className="max-w-3xl w-full mx-auto space-y-6">
      <div className="space-y-2">
        <div className="px-4">
          <h2 className="text-xs font-medium text-mid-gray uppercase tracking-wide">
            {t("settings.analytics.title")}
          </h2>
        </div>

        <div className="bg-background border border-mid-gray/20 rounded-lg overflow-visible">
          <PeriodSelector period={period} onPeriodChange={setPeriod} />
          {renderContent()}
        </div>
      </div>
    </div>
  );
};

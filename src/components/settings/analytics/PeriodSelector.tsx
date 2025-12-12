import React from "react";
import { useTranslation } from "react-i18next";

export type Period = "today" | "this_week" | "all_time";

interface PeriodSelectorProps {
  period: Period;
  onPeriodChange: (period: Period) => void;
}

const periods: { key: Period; labelKey: string }[] = [
  { key: "today", labelKey: "settings.analytics.periods.today" },
  { key: "this_week", labelKey: "settings.analytics.periods.thisWeek" },
  { key: "all_time", labelKey: "settings.analytics.periods.allTime" },
];

export const PeriodSelector: React.FC<PeriodSelectorProps> = ({
  period,
  onPeriodChange,
}) => {
  const { t } = useTranslation();

  return (
    <div className="px-4 py-3 flex gap-2 border-b border-mid-gray/20">
      {periods.map((p) => (
        <button
          key={p.key}
          onClick={() => onPeriodChange(p.key)}
          className={`px-3 py-1.5 text-sm rounded-md transition-colors cursor-pointer ${
            period === p.key
              ? "bg-logo-primary text-white"
              : "bg-mid-gray/20 hover:bg-mid-gray/30"
          }`}
        >
          {t(p.labelKey)}
        </button>
      ))}
    </div>
  );
};

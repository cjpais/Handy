import type { TFunction } from "i18next";

export const formatStudioBytes = (bytes: number, t: TFunction) => {
  if (!bytes) {
    return t("studio.common.unknownSize", { defaultValue: "Unknown size" });
  }

  const gb = bytes / 1024 / 1024 / 1024;
  if (gb >= 1) {
    return new Intl.NumberFormat(undefined, {
      style: "unit",
      unit: "gigabyte",
      unitDisplay: "narrow",
      maximumFractionDigits: 1,
    }).format(gb);
  }

  const mb = bytes / 1024 / 1024;
  return new Intl.NumberFormat(undefined, {
    style: "unit",
    unit: "megabyte",
    unitDisplay: "narrow",
    maximumFractionDigits: 0,
  }).format(mb);
};

export const formatStudioDuration = (durationMs: number) => {
  const totalSeconds = Math.max(0, Math.round(durationMs / 1000));
  const hours = Math.floor(totalSeconds / 3600);
  const minutes = Math.floor((totalSeconds % 3600) / 60);
  const seconds = totalSeconds % 60;

  const hourFormatter = new Intl.NumberFormat(undefined, {
    style: "unit",
    unit: "hour",
    unitDisplay: "narrow",
    maximumFractionDigits: 0,
  });
  const minuteFormatter = new Intl.NumberFormat(undefined, {
    style: "unit",
    unit: "minute",
    unitDisplay: "narrow",
    maximumFractionDigits: 0,
  });
  const secondFormatter = new Intl.NumberFormat(undefined, {
    style: "unit",
    unit: "second",
    unitDisplay: "narrow",
    maximumFractionDigits: 0,
  });

  const parts: string[] = [];
  if (hours > 0) {
    parts.push(hourFormatter.format(hours));
  }
  if (minutes > 0 || hours > 0) {
    parts.push(minuteFormatter.format(minutes));
  }
  parts.push(secondFormatter.format(seconds));

  return parts.join(" ");
};

export const formatStudioImportedAt = (timestamp: number) => {
  const date = new Date(timestamp);
  const now = new Date();
  const sameDay = date.toDateString() === now.toDateString();

  return new Intl.DateTimeFormat(undefined, {
    ...(sameDay
      ? {
          hour: "numeric",
          minute: "2-digit",
          second: "2-digit",
        }
      : {
          month: "short",
          day: "numeric",
          hour: "numeric",
          minute: "2-digit",
        }),
  }).format(date);
};

export const formatStudioEstimate = (
  min: number | null,
  max: number | null,
  t: TFunction,
) => {
  if (min == null || max == null) {
    return t("studio.common.estimateFallback", {
      defaultValue: "About a few minutes",
    });
  }
  return t("studio.common.estimateRange", {
    defaultValue: "About {{min}} to {{max}} minutes",
    min,
    max,
  });
};

export const formatStudioRelativeTime = (timestamp: number) => {
  const diff = Date.now() - timestamp;
  const minutes = Math.floor(diff / 60000);
  const formatter = new Intl.RelativeTimeFormat(undefined, {
    numeric: "auto",
  });

  if (minutes < 1) return formatter.format(0, "minute");
  if (minutes < 60) return formatter.format(-minutes, "minute");

  const hours = Math.floor(minutes / 60);
  if (hours < 24) return formatter.format(-hours, "hour");

  const days = Math.floor(hours / 24);
  return formatter.format(-days, "day");
};

import React, { useRef } from "react";
import { useTranslation } from "react-i18next";
import { DoneMark } from "@/components/shared/DoneMark";
import { ProgressRing } from "@/components/shared/ProgressRing";
import { BYTES_PER_MB, formatModelSize } from "@/lib/utils/format";
import { useOnboardingStore } from "./onboardingStore";
import type { DownloadState, useOnboardingModels } from "./useOnboardingModels";
import type { CatalogModel } from "./data/models";

type Downloading = Extract<DownloadState, { state: "downloading" }>;

/**
 * A download's numbers in words: how far ("297 MB of 706 MB"), the time
 * left and the speed, as the Models page's cards give them.
 */
export const useDownloadWords = () => {
  const { t, i18n } = useTranslation();
  // At least a byte: formatModelSize calls 0 an unknown size.
  const bytes = (n: number) => formatModelSize(Math.max(n, 1) / BYTES_PER_MB);
  return {
    /** Whole percent, never 100 before it's done. */
    percent: (d: Downloading) => Math.min(99, Math.floor(d.progress * 100)),
    /** "297 MB of 706 MB"; the catalog's size until the download knows. */
    sizes: (d: Downloading, sizeMb: number | undefined) => ({
      done: bytes(d.downloaded),
      total: d.total > 0 ? bytes(d.total) : formatModelSize(sizeMb),
    }),
    /** "3 min", in the app's language (the browser localizes the unit);
     *  takes seconds left (ModelMigration sums several downloads). */
    time: (etaSeconds: number | null) => {
      if (etaSeconds == null) return null;
      const minutes = etaSeconds >= 60;
      return new Intl.NumberFormat(i18n.language, {
        style: "unit",
        unit: minutes ? "minute" : "second",
        unitDisplay: "short",
      }).format(Math.ceil(minutes ? etaSeconds / 60 : etaSeconds));
    },
    /** "4.2 MB/s"; takes bytes per second. */
    speed: (bytesPerSec: number | null) =>
      bytesPerSec == null
        ? null
        : t("modelSelector.downloadSpeed", {
            speed: (bytesPerSec / BYTES_PER_MB).toFixed(1),
          }),
  };
};

/**
 * The download, beside the button it's holding back (Demo): a ring, what's
 * happening, then how far. Only the first line is announced, so a screen
 * reader hears each change of state, not every megabyte. "Ready" shows only
 * for a download that finished here, not a model that was already on disk.
 */
export const DownloadStatus: React.FC<{
  models: ReturnType<typeof useOnboardingModels>;
  model: CatalogModel | undefined;
}> = ({ models, model }) => {
  const { t } = useTranslation();
  const words = useDownloadWords();
  const preview = useOnboardingStore((s) => s.preview);
  const d = preview
    ? ({ state: "none" } as const)
    : models.downloadState(model);

  // Seen loading on this screen: noted as it renders, so "ready" shows in
  // the same render the download finishes, on the same node.
  const loaded = useRef(false);
  if (d.state === "downloading" || d.state === "checking")
    loaded.current = true;

  if (!model || d.state === "none" || (d.state === "ready" && !loaded.current))
    return null;
  const name = model.name;

  let icon: React.ReactNode;
  let one: string;
  let two: string | null = null;
  let tone = "text-text";
  switch (d.state) {
    case "downloading": {
      const { done, total } = words.sizes(d, models.infoFor(model)?.size_mb);
      const time = words.time(d.etaSeconds);
      icon = <ProgressRing value={d.progress} size={20} />;
      one = t("onboarding.download.downloading", { name });
      two = time
        ? t("onboarding.download.progressLeft", { done, total, time })
        : t("onboarding.download.progress", { done, total });
      break;
    }
    case "checking":
      icon = <ProgressRing value={1} size={20} />;
      one = t("onboarding.download.checkingName", { name });
      two = t("onboarding.download.checkingDetail");
      break;
    case "failed":
      icon = (
        <span
          aria-hidden="true"
          className="grid size-[16px] shrink-0 place-items-center rounded-full bg-warning text-[11px] font-bold leading-none text-background"
        >
          !
        </span>
      );
      one = t("onboarding.download.stopped", { name });
      two = t("onboarding.download.retryDetail");
      tone = "text-warning";
      break;
    default:
      icon = <DoneMark />;
      one = t("onboarding.download.ready", { name });
      tone = "text-moss";
  }

  return (
    <div className="flex min-w-0 items-center gap-[10px] text-start">
      {icon}
      <div className="min-w-0 leading-[1.35]">
        <div
          role="status"
          className={`truncate text-small font-medium ${tone}`}
        >
          {one}
        </div>
        {two && (
          <div className="truncate text-label tabular-nums text-ink-3">
            {two}
          </div>
        )}
      </div>
    </div>
  );
};

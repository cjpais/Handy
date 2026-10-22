import React from "react";
import { DoneMark } from "./DoneMark";
import { ProgressRing } from "./ProgressRing";

interface DownloadPillProps {
  state: "downloading" | "checking" | "ready" | "failed";
  /** 0–1, while downloading. */
  progress?: number;
  /** "Name, 1 min left" / "Checking the download…" / "Name is ready". */
  text: string;
  title?: string;
  /** After the text: a retry when it failed. */
  action?: React.ReactNode;
}

/**
 * The model download, top right after Model: a ring, then a moss check; a
 * warning and a retry if it stops.
 */
export const DownloadPill: React.FC<DownloadPillProps> = ({
  state,
  progress = 0,
  text,
  title,
  action,
}) => {
  const ready = state === "ready";
  const failed = state === "failed";
  return (
    <div
      role="status"
      aria-live="polite"
      title={title}
      className={`flex items-center gap-[8px] h-[28px] max-w-[340px] ps-[6px] pe-[11px] rounded-full border text-label tabular-nums whitespace-nowrap ${
        ready
          ? "text-moss bg-moss-wash border-[color-mix(in_srgb,var(--color-moss)_25%,transparent)]"
          : failed
            ? "ps-[11px] text-warning bg-wash border-line"
            : "text-ink-2 bg-wash border-line"
      }`}
    >
      {ready ? (
        <DoneMark className="ms-[2px]" />
      ) : failed ? null : (
        <ProgressRing value={state === "checking" ? 1 : progress} />
      )}
      <span className="truncate">{text}</span>
      {action}
    </div>
  );
};

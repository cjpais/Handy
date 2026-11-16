import React from "react";
import { useTranslation } from "react-i18next";
import { useTranscriptionJobsStore } from "@/stores/transcriptionJobsStore";

interface TranscriptionStatusProps {
  /** Where the jobs and their progress are. */
  onShowHistory: () => void;
}

/** Background transcription beside the model doing it; opens History. */
const TranscriptionStatus: React.FC<TranscriptionStatusProps> = ({
  onShowHistory,
}) => {
  const { t } = useTranslation();
  const jobs = Object.values(useTranscriptionJobsStore((s) => s.jobs));
  if (jobs.length === 0) return null;

  // Jobs run one at a time; the rest wait behind it.
  const running = jobs.find((j) => j.state === "running");
  const text = !running
    ? t("settings.history.waiting")
    : running.percent > 0
      ? t("settings.history.transcribingPercent", { percent: running.percent })
      : t("settings.history.transcribing");

  return (
    <button
      type="button"
      onClick={onShowHistory}
      className="cursor-pointer tabular-nums text-ink-3 transition-colors hover:text-text focus:outline-none focus-visible:ring-1 focus-visible:ring-logo-primary"
    >
      {text}
    </button>
  );
};

export default TranscriptionStatus;

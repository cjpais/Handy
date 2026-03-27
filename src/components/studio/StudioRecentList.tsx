import React from "react";
import { useTranslation } from "react-i18next";
import { FolderOpen, RotateCcw, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/Button";
import type { StudioJob } from "@/lib/types/studio";

interface StudioRecentListProps {
  jobs: StudioJob[];
  onOpenFolder: (jobId: string) => Promise<void>;
  onRetry: (jobId: string) => Promise<void>;
  onDelete: (jobId: string) => Promise<void>;
}

const formatRelativeTime = (timestamp: number) => {
  const diff = Date.now() - timestamp;
  const minutes = Math.floor(diff / 60000);
  if (minutes < 1) return "just now";
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h ago`;
  const days = Math.floor(hours / 24);
  return `${days}d ago`;
};

export const StudioRecentList: React.FC<StudioRecentListProps> = ({
  jobs,
  onOpenFolder,
  onRetry,
  onDelete,
}) => {
  const { t } = useTranslation();

  if (jobs.length === 0) {
    return null;
  }

  const statusLabel = (status: StudioJob["status"]) =>
    t(`studio.statuses.${status}`, {
      defaultValue:
        {
          pending: "Ready",
          running: "Running",
          paused: "Paused",
          done: "Done",
          error: "Failed",
          cancelled: "Cancelled",
        }[status] ?? status,
    });

  return (
    <div className="rounded-2xl border border-mid-gray/20 bg-background p-5">
      <div className="mb-3 flex items-center justify-between">
        <h3 className="text-sm font-semibold uppercase tracking-wide text-text/60">
          {t("studio.recent.title", { defaultValue: "Recent Jobs" })}
        </h3>
      </div>
      <div className="space-y-3">
        {jobs.map((job) => (
          <div
            key={job.id}
            className="flex flex-col gap-3 rounded-xl border border-mid-gray/15 bg-mid-gray/5 p-3"
          >
            <div className="flex items-start justify-between gap-3">
              <div>
                <p className="text-sm font-medium">{job.source_name}</p>
                <p className="text-xs text-text/55">
                  {statusLabel(job.status)} - {formatRelativeTime(job.created_at)}
                </p>
              </div>
              <div className="flex items-center gap-2">
                {job.output_folder && job.status === "done" && (
                  <Button
                    variant="ghost"
                    size="sm"
                    onClick={() => onOpenFolder(job.id)}
                    title={t("studio.job.openFolder", { defaultValue: "Open output folder" })}
                    aria-label={t("studio.job.openFolder", { defaultValue: "Open output folder" })}
                  >
                    <FolderOpen className="h-4 w-4" />
                  </Button>
                )}
                {(job.status === "error" || job.status === "cancelled") && (
                  <Button
                    variant="ghost"
                    size="sm"
                    onClick={() => onRetry(job.id)}
                    title={t("studio.job.retry", { defaultValue: "Retry job" })}
                    aria-label={t("studio.job.retry", { defaultValue: "Retry job" })}
                  >
                    <RotateCcw className="h-4 w-4" />
                  </Button>
                )}
                <Button
                  variant="danger-ghost"
                  size="sm"
                  onClick={() => onDelete(job.id)}
                  title={t("studio.recent.delete", { defaultValue: "Delete job" })}
                  aria-label={t("studio.recent.delete", { defaultValue: "Delete job" })}
                >
                  <Trash2 className="h-4 w-4" />
                </Button>
              </div>
            </div>
            {job.error_message && (
              <p className="text-xs text-red-400">{job.error_message}</p>
            )}
          </div>
        ))}
      </div>
    </div>
  );
};

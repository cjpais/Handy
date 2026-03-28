import React from "react";
import { useTranslation } from "react-i18next";
import { confirm } from "@tauri-apps/plugin-dialog";
import { Filter, FolderOpen, RotateCcw, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/Button";
import type { StudioJob } from "@/lib/types/studio";

interface StudioRecentListProps {
  jobs: StudioJob[];
  onSelectJob: (jobId: string) => Promise<void>;
  onOpenFolder: (jobId: string) => Promise<void>;
  onRetry: (jobId: string) => Promise<void>;
  onDelete: (jobId: string) => Promise<void>;
  onClearAll: (jobIds: string[]) => Promise<void>;
  selectedJobId: string | null;
  selectionToken: number;
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

const formatDuration = (durationMs: number) => {
  const totalSeconds = Math.round(durationMs / 1000);
  const minutes = Math.floor(totalSeconds / 60);
  const seconds = totalSeconds % 60;
  return `${minutes}m ${seconds}s`;
};

const formatBytes = (bytes: number) => {
  if (!bytes) return "Unknown size";
  const mb = bytes / 1024 / 1024;
  return `${mb.toFixed(0)} MB`;
};

const formatImportedAt = (timestamp: number) => {
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

export const StudioRecentList: React.FC<StudioRecentListProps> = ({
  jobs,
  onSelectJob,
  onOpenFolder,
  onRetry,
  onDelete,
  onClearAll,
  selectedJobId,
  selectionToken,
}) => {
  const { t } = useTranslation();
  const [statusFilter, setStatusFilter] = React.useState<
    StudioJob["status"] | "all"
  >("all");
  const [isFilterOpen, setIsFilterOpen] = React.useState(false);

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

  const statusOrder: StudioJob["status"][] = [
    "pending",
    "running",
    "paused",
    "done",
    "error",
    "cancelled",
  ];
  const availableStatuses = React.useMemo(
    () =>
      statusOrder.filter((status) => jobs.some((job) => job.status === status)),
    [jobs],
  );

  React.useEffect(() => {
    if (statusFilter !== "all" && !availableStatuses.includes(statusFilter)) {
      setStatusFilter("all");
    }
  }, [availableStatuses, statusFilter]);

  React.useEffect(() => {
    if (statusFilter === "all" && availableStatuses.length <= 1) {
      setIsFilterOpen(false);
    }
  }, [availableStatuses.length, statusFilter]);

  const filteredJobs = React.useMemo(
    () =>
      statusFilter === "all"
        ? jobs
        : jobs.filter((job) => job.status === statusFilter),
    [jobs, statusFilter],
  );
  const clearableJobIds = React.useMemo(
    () =>
      jobs
        .filter((job) => job.status !== "running" && job.status !== "paused")
        .map((job) => job.id),
    [jobs],
  );
  const shouldShowFilterButton = availableStatuses.length > 1;
  const shouldShowClearAllButton = clearableJobIds.length > 0;
  const isFilterActive = statusFilter !== "all";
  const showFilterPanel =
    shouldShowFilterButton && (isFilterOpen || isFilterActive);

  if (jobs.length === 0) {
    return null;
  }

  const handleSelectJob = async (jobId: string) => {
    await onSelectJob(jobId);
  };

  const handleClearAll = async () => {
    const count = clearableJobIds.length;
    if (count === 0) return;

    const hasActiveJob = jobs.some(
      (job) => job.status === "running" || job.status === "paused",
    );
    const confirmed = await confirm(
      hasActiveJob
        ? t("studio.recent.clearAllConfirmWithActive", {
            defaultValue:
              "Delete {{count}} recent jobs? Running or paused jobs will be kept.",
            count,
          })
        : t("studio.recent.clearAllConfirm", {
            defaultValue: "Delete {{count}} recent jobs?",
            count,
          }),
      {
        title: t("studio.recent.clearAllTitle", {
          defaultValue: "Clear recent jobs",
        }),
        kind: "warning",
        okLabel: t("studio.recent.clearAllAction", {
          defaultValue: "Delete all",
        }),
        cancelLabel: t("common.cancel", { defaultValue: "Cancel" }),
      },
    );
    if (!confirmed) return;

    await onClearAll(clearableJobIds);
  };

  const handleToggleFilters = () => {
    if (showFilterPanel) {
      setStatusFilter("all");
      setIsFilterOpen(false);
      return;
    }

    setIsFilterOpen(true);
  };

  return (
    <div className="rounded-2xl border border-mid-gray/20 bg-background p-5">
      <div className="mb-3 flex items-center justify-between">
        <h3 className="text-sm font-semibold uppercase tracking-wide text-text/60">
          {t("studio.recent.title", { defaultValue: "Recent Jobs" })}
        </h3>
        <div className="flex items-center gap-1">
          {shouldShowFilterButton && (
            <Button
              variant={showFilterPanel ? "primary-soft" : "ghost"}
              size="sm"
              className="relative px-2"
              onClick={handleToggleFilters}
              aria-label={t("studio.recent.filters.toggle", {
                defaultValue: "Filter recent jobs",
              })}
              aria-pressed={showFilterPanel}
            >
              <Filter className="h-4 w-4" />
              {isFilterActive && (
                <span className="absolute right-1 top-1 h-1.5 w-1.5 rounded-full bg-logo-primary" />
              )}
            </Button>
          )}
          {shouldShowClearAllButton && (
            <Button
              variant="danger-ghost"
              size="sm"
              className="px-2"
              onClick={() => void handleClearAll()}
              aria-label={t("studio.recent.clearAll", {
                defaultValue: "Clear all recent jobs",
              })}
            >
              <Trash2 className="h-4 w-4" />
            </Button>
          )}
        </div>
      </div>
      {showFilterPanel && (
        <div className="mb-4 flex flex-wrap gap-2">
          <Button
            variant={statusFilter === "all" ? "primary-soft" : "secondary"}
            size="sm"
            onClick={() => {
              setStatusFilter("all");
              setIsFilterOpen(false);
            }}
          >
            {t("studio.recent.filters.all", { defaultValue: "All" })}
          </Button>
          {availableStatuses.map((status) => (
            <Button
              key={status}
              variant={statusFilter === status ? "primary-soft" : "secondary"}
              size="sm"
              onClick={() => setStatusFilter(status)}
            >
              {statusLabel(status)}
            </Button>
          ))}
        </div>
      )}
      <div className="space-y-3">
        {filteredJobs.length === 0 && (
          <div className="rounded-xl border border-dashed border-mid-gray/20 bg-mid-gray/5 px-4 py-5 text-sm text-text/55">
            {t("studio.recent.emptyFiltered", {
              defaultValue: "No recent jobs match this status.",
            })}
          </div>
        )}
        {filteredJobs.map((job) => (
          <div
            key={
              job.id === selectedJobId ? `${job.id}-${selectionToken}` : job.id
            }
            className={`flex flex-col gap-3 rounded-xl border border-mid-gray/15 bg-mid-gray/5 p-3 transition-all ${"cursor-pointer hover:border-logo-primary/40"} ${
              job.id === selectedJobId
                ? "studio-recent-selected studio-recent-pulse"
                : ""
            }`}
            onClick={() => void handleSelectJob(job.id)}
            onKeyDown={(event) => {
              if (event.key === "Enter" || event.key === " ") {
                event.preventDefault();
                void handleSelectJob(job.id);
              }
            }}
            role="button"
            tabIndex={0}
            aria-pressed={job.id === selectedJobId}
          >
            <div className="flex items-start justify-between gap-3">
              <div>
                <div className="flex flex-wrap items-center gap-2">
                  <p className="text-sm font-medium">{job.source_name}</p>
                  {job.id === selectedJobId && (
                    <span className="rounded-full bg-logo-primary/15 px-2 py-0.5 text-[11px] font-medium text-logo-primary">
                      {t("studio.recent.viewing", { defaultValue: "Viewing" })}
                    </span>
                  )}
                </div>
                <p className="text-xs text-text/55">
                  {statusLabel(job.status)} -{" "}
                  {formatRelativeTime(job.created_at)}
                </p>
                <p className="mt-1 text-xs text-text/45">
                  {t("studio.recent.details", {
                    defaultValue: "{{duration}} - {{size}} - Imported {{time}}",
                    duration: formatDuration(job.media_duration_ms),
                    size: formatBytes(job.file_size_bytes),
                    time: formatImportedAt(job.created_at),
                  })}
                </p>
              </div>
              <div className="flex items-center gap-2">
                {job.output_folder && job.status === "done" && (
                  <Button
                    variant="ghost"
                    size="sm"
                    onClick={(event) => {
                      event.stopPropagation();
                      void onOpenFolder(job.id);
                    }}
                    title={t("studio.job.openFolder", {
                      defaultValue: "Open output folder",
                    })}
                    aria-label={t("studio.job.openFolder", {
                      defaultValue: "Open output folder",
                    })}
                  >
                    <FolderOpen className="h-4 w-4" />
                  </Button>
                )}
                {(job.status === "error" || job.status === "cancelled") && (
                  <Button
                    variant="ghost"
                    size="sm"
                    onClick={(event) => {
                      event.stopPropagation();
                      void onRetry(job.id);
                    }}
                    title={t("studio.job.retry", { defaultValue: "Retry job" })}
                    aria-label={t("studio.job.retry", {
                      defaultValue: "Retry job",
                    })}
                  >
                    <RotateCcw className="h-4 w-4" />
                  </Button>
                )}
                {job.status !== "running" && job.status !== "paused" && (
                  <Button
                    variant="danger-ghost"
                    size="sm"
                    onClick={(event) => {
                      event.stopPropagation();
                      void onDelete(job.id);
                    }}
                    title={t("studio.recent.delete", {
                      defaultValue: "Delete job",
                    })}
                    aria-label={t("studio.recent.delete", {
                      defaultValue: "Delete job",
                    })}
                  >
                    <Trash2 className="h-4 w-4" />
                  </Button>
                )}
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

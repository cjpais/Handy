import React from "react";
import { useTranslation } from "react-i18next";
import { confirm } from "@tauri-apps/plugin-dialog";
import { FolderOpen, RotateCcw, Square } from "lucide-react";
import { Button } from "@/components/ui/Button";
import { Alert } from "@/components/ui/Alert";
import {
  formatStudioBytes,
  formatStudioDuration,
  formatStudioImportedAt,
} from "@/lib/studioFormat";
import type { StudioJob } from "@/lib/types/studio";

interface StudioJobViewProps {
  job: StudioJob;
  statusMessage: string | null;
  stage: string;
  preparationProgress: number | null;
  error: string | null;
  loadedFromRecent?: boolean;
  onCancel: () => Promise<void>;
  onRetry: () => Promise<void>;
  onOpenFolder: () => Promise<void>;
}

const progressPercentage = (job: StudioJob) => {
  if (job.chunk_count <= 0) return 0;
  return Math.max(
    0,
    Math.min(100, Math.round((job.chunks_completed / job.chunk_count) * 100)),
  );
};

const PREPARATION_STAGES = new Set([
  "preparing_audio",
  "opening_file",
  "decoding_audio",
  "resampling_audio",
  "writing_normalized_audio",
  "building_chunks",
]);

export const StudioJobView: React.FC<StudioJobViewProps> = ({
  job,
  statusMessage,
  stage,
  preparationProgress,
  error,
  loadedFromRecent = false,
  onCancel,
  onRetry,
  onOpenFolder,
}) => {
  const { t } = useTranslation();
  const isRunning = job.status === "running" || job.status === "paused";
  const isDone = job.status === "done";
  const isError = job.status === "error" || job.status === "cancelled";
  const fallbackStatusMessage = isDone
    ? t("studio.statuses.done", { defaultValue: "Done" })
    : job.status === "error"
      ? t("studio.statuses.error", { defaultValue: "Failed" })
      : job.status === "cancelled"
        ? t("studio.statuses.cancelled", { defaultValue: "Cancelled" })
        : job.status === "paused"
          ? t("studio.statuses.paused", { defaultValue: "Paused" })
          : job.status === "running"
            ? t("studio.statuses.running", { defaultValue: "Running" })
            : job.status === "pending"
              ? t("studio.statuses.pending", { defaultValue: "Ready" })
              : t("studio.job.waiting", { defaultValue: "Waiting" });
  const isPreparing = PREPARATION_STAGES.has(stage);
  const preparationValue =
    isRunning && isPreparing && preparationProgress !== null
      ? Math.max(0, Math.min(100, preparationProgress))
      : null;
  const isIndeterminate =
    isRunning &&
    isPreparing &&
    job.chunk_count <= 0 &&
    preparationValue === null;
  const progressLabel =
    preparationValue !== null
      ? `${preparationValue}%`
      : isIndeterminate
        ? t("studio.job.preparing", { defaultValue: "Preparing..." })
        : `${job.chunks_completed} / ${job.chunk_count || "?"}`;
  const isStopping = stage === "stopping";
  const resolvedStatusMessage = (() => {
    if (stage === "stopping") {
      return t("studio.job.status.stopping", { defaultValue: "Stopping..." });
    }

    switch (stage) {
      case "preparing_audio":
        return t("studio.job.stage.preparingAudio", {
          defaultValue: "Preparing audio",
        });
      case "opening_file":
        return t("studio.job.stage.openingFile", {
          defaultValue: "Opening file",
        });
      case "decoding_audio":
        return t("studio.job.stage.decodingAudio", {
          defaultValue: "Decoding audio",
        });
      case "resampling_audio":
        return t("studio.job.stage.resamplingAudio", {
          defaultValue: "Resampling audio",
        });
      case "writing_normalized_audio":
        return t("studio.job.stage.writingNormalizedAudio", {
          defaultValue: "Writing normalized audio",
        });
      case "building_chunks":
        return t("studio.job.stage.buildingChunks", {
          defaultValue: "Building chunks",
        });
      case "transcribing":
        return t("studio.job.stage.transcribing", {
          defaultValue: "Transcribing audio",
        });
      case "writing_output_files":
        return t("studio.job.stage.writingOutputFiles", {
          defaultValue: "Writing output files",
        });
      case "paused":
        return t("studio.job.status.pausedForDictation", {
          defaultValue: "Paused while dictation is running",
        });
      case "done":
      case "error":
      case "idle":
      case "ready":
      default:
        return statusMessage || fallbackStatusMessage;
    }
  })();

  const handleCancel = async () => {
    const confirmed = await confirm(
      t("studio.job.confirmCancel", {
        defaultValue:
          "Stop this Studio job now? You can retry it later from the recent jobs list.",
      }),
      {
        title: t("studio.job.stop", { defaultValue: "Stop job" }),
        kind: "warning",
        okLabel: t("studio.job.stop", { defaultValue: "Stop job" }),
        cancelLabel: t("common.cancel", { defaultValue: "Cancel" }),
      },
    );
    if (!confirmed) return;
    await onCancel();
  };

  return (
    <div
      className={`rounded-2xl border border-mid-gray/20 bg-background p-5 ${
        loadedFromRecent ? "studio-setup-loaded" : ""
      }`}
    >
      <div className="flex flex-col gap-4">
        {loadedFromRecent && (
          <div className="rounded-xl border border-logo-primary/30 bg-logo-primary/10 px-3 py-2 text-xs text-text/70">
            <span className="font-medium text-text">
              {t("studio.job.loadedFromRecent", {
                defaultValue: "Viewing from Recent Jobs",
              })}
            </span>
            <span className="ml-2 text-text/55">
              {t("studio.job.importedAt", {
                defaultValue: "Imported {{value}}",
                value: formatStudioImportedAt(job.created_at),
              })}
            </span>
          </div>
        )}

        <div className="flex flex-wrap items-start justify-between gap-3">
          <div>
            <h2 className="text-lg font-semibold">{job.source_name}</h2>
            <p className="mt-1 text-sm text-text/60">{resolvedStatusMessage}</p>
          </div>
          <div className="flex gap-2">
            {isRunning && (
              <Button
                variant="danger"
                onClick={handleCancel}
                disabled={isStopping}
                title={t("studio.job.stop", { defaultValue: "Stop job" })}
                aria-label={t("studio.job.stop", { defaultValue: "Stop job" })}
              >
                <Square className="h-4 w-4" />
              </Button>
            )}
            {isDone && (
              <Button
                variant="secondary"
                onClick={onOpenFolder}
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
            {isError && (
              <Button
                variant="secondary"
                onClick={onRetry}
                title={t("studio.job.retry", { defaultValue: "Retry job" })}
                aria-label={t("studio.job.retry", {
                  defaultValue: "Retry job",
                })}
              >
                <RotateCcw className="h-4 w-4" />
              </Button>
            )}
          </div>
        </div>

        <div className="grid gap-2 text-sm text-text/65 sm:grid-cols-2">
          <p>
            {t("studio.job.duration", {
              defaultValue: "Duration: {{value}}",
              value: formatStudioDuration(job.media_duration_ms),
            })}
          </p>
          <p>
            {t("studio.job.size", {
              defaultValue: "Size: {{value}}",
              value: formatStudioBytes(job.file_size_bytes, t),
            })}
          </p>
          <p>
            {t("studio.job.estimate", {
              defaultValue: "Estimate: {{value}}",
              value:
                job.estimate_text ||
                t("studio.common.estimateFallback", {
                  defaultValue: "About a few minutes",
                }),
            })}
          </p>
          <p>
            {t("studio.job.currentStep", {
              defaultValue: "Current step: {{value}}",
              value: resolvedStatusMessage,
            })}
          </p>
        </div>

        <div>
          <div className="mb-2 flex items-center justify-between text-sm text-text/60">
            <span>
              {stage === "paused"
                ? t("studio.job.paused", { defaultValue: "Paused" })
                : t("studio.job.progress", { defaultValue: "Progress" })}
            </span>
            <span>{progressLabel}</span>
          </div>
          <div className="h-2 rounded-full bg-mid-gray/15">
            {isIndeterminate ? (
              <div className="h-2 w-2/5 rounded-full bg-logo-primary animate-pulse" />
            ) : (
              <div
                className="h-2 rounded-full bg-logo-primary transition-all"
                style={{
                  width: `${preparationValue ?? progressPercentage(job)}%`,
                }}
              />
            )}
          </div>
        </div>

        {(error || job.error_message) && (
          <Alert variant="error">
            {error ||
              job.error_message ||
              t("studio.job.processError", {
                defaultValue: "This file could not be processed",
              })}
          </Alert>
        )}

        <div className="rounded-xl border border-mid-gray/15 bg-mid-gray/5 p-4">
          <p className="mb-2 text-sm font-medium">
            {t("studio.job.preview", { defaultValue: "Transcript preview" })}
          </p>
          <div className="max-h-72 overflow-y-auto whitespace-pre-wrap text-sm text-text/75">
            {job.transcript_text.trim() ||
              t("studio.job.previewEmpty", {
                defaultValue: "Transcript preview will appear here.",
              })}
          </div>
        </div>

        {isDone && job.output_files.length > 0 && (
          <div className="space-y-2">
            <p className="text-sm font-medium">
              {t("studio.job.outputFiles", { defaultValue: "Output files" })}
            </p>
            <div className="flex flex-wrap gap-2">
              {job.output_files.map((file) => (
                <span
                  key={file.output_path}
                  className="rounded-full bg-mid-gray/10 px-3 py-1 text-xs text-text/70"
                >
                  {file.file_name}
                </span>
              ))}
            </div>
          </div>
        )}
      </div>
    </div>
  );
};

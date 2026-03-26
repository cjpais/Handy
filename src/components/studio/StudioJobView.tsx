import React from "react";
import { FolderOpen, RotateCcw, Square } from "lucide-react";
import { Button } from "@/components/ui/Button";
import { Alert } from "@/components/ui/Alert";
import type { StudioJob } from "@/lib/types/studio";

interface StudioJobViewProps {
  job: StudioJob;
  statusMessage: string | null;
  stage: string;
  error: string | null;
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

export const StudioJobView: React.FC<StudioJobViewProps> = ({
  job,
  statusMessage,
  stage,
  error,
  onCancel,
  onRetry,
  onOpenFolder,
}) => {
  const isRunning = job.status === "running" || job.status === "paused";
  const isDone = job.status === "done";
  const isError = job.status === "error" || job.status === "cancelled";

  return (
    <div className="rounded-2xl border border-mid-gray/20 bg-background p-5">
      <div className="flex flex-col gap-4">
        <div className="flex flex-wrap items-start justify-between gap-3">
          <div>
            <h2 className="text-lg font-semibold">{job.source_name}</h2>
            <p className="mt-1 text-sm text-text/60">
              {statusMessage || (isDone ? "Done" : "Waiting")}
            </p>
          </div>
          <div className="flex gap-2">
            {isRunning && (
              <Button variant="danger" onClick={onCancel}>
                <Square className="h-4 w-4" />
              </Button>
            )}
            {isDone && (
              <Button variant="secondary" onClick={onOpenFolder}>
                <FolderOpen className="h-4 w-4" />
              </Button>
            )}
            {isError && (
              <Button variant="secondary" onClick={onRetry}>
                <RotateCcw className="h-4 w-4" />
              </Button>
            )}
          </div>
        </div>

        <div>
          <div className="mb-2 flex items-center justify-between text-sm text-text/60">
            <span>{stage === "paused" ? "Paused" : "Progress"}</span>
            <span>
              {job.chunks_completed} / {job.chunk_count || "?"}
            </span>
          </div>
          <div className="h-2 rounded-full bg-mid-gray/15">
            <div
              className="h-2 rounded-full bg-logo-primary transition-all"
              style={{ width: `${progressPercentage(job)}%` }}
            />
          </div>
        </div>

        {(error || job.error_message) && (
          <Alert variant="error">
            {error || job.error_message || "This file could not be processed"}
          </Alert>
        )}

        <div className="rounded-xl border border-mid-gray/15 bg-mid-gray/5 p-4">
          <p className="mb-2 text-sm font-medium">Transcript preview</p>
          <div className="max-h-72 overflow-y-auto whitespace-pre-wrap text-sm text-text/75">
            {job.transcript_text.trim() || "Transcript preview will appear here."}
          </div>
        </div>

        {isDone && job.output_files.length > 0 && (
          <div className="space-y-2">
            <p className="text-sm font-medium">Output files</p>
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

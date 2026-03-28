import React from "react";
import { useTranslation } from "react-i18next";
import { open } from "@tauri-apps/plugin-dialog";
import { UploadCloud } from "lucide-react";
import { toast } from "sonner";
import { Alert } from "@/components/ui/Alert";
import { Button } from "@/components/ui/Button";
import { useSettings } from "@/hooks/useSettings";
import { useStudioStore } from "@/stores/studioStore";
import { StudioDropzone } from "./StudioDropzone";
import { StudioJobView } from "./StudioJobView";
import { StudioRecentList } from "./StudioRecentList";
import { StudioSetupCard } from "./StudioSetupCard";

export const StudioHome: React.FC = () => {
  const { t } = useTranslation();
  const { settings } = useSettings();
  const initializeStudio = useStudioStore((state) => state.initialize);
  const studio = useStudioStore();

  React.useEffect(() => {
    initializeStudio().catch((error) => {
      console.error("Failed to initialize Studio:", error);
      toast.error(
        t("studio.errors.initializeFailed", {
          defaultValue: "Failed to initialize Studio",
        }),
        {
          description: error instanceof Error ? error.message : String(error),
        },
      );
    });
  }, [initializeStudio, t]);

  const [outputFolder, setOutputFolder] = React.useState(
    studio.lastOutputFolder,
  );

  React.useEffect(() => {
    if (studio.preparedJob) {
      const fallbackFolder =
        studio.lastOutputFolder ||
        studio.preparedJob.output_folder ||
        studio.preparedJob.source_dir ||
        "";
      setOutputFolder(fallbackFolder);
    }
  }, [studio.preparedJob, studio.lastOutputFolder]);

  const handlePrepare = async (filePath: string) => {
    try {
      await studio.prepareFile(filePath);
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      toast.error(message);
    }
  };

  const handleChooseAnotherFile = async () => {
    const selected = await open({
      multiple: false,
      directory: false,
      filters: [{ name: "Audio", extensions: studio.supportedExtensions }],
    });

    if (typeof selected === "string") {
      await handlePrepare(selected);
    }
  };

  const handleStart = async () => {
    try {
      await studio.startPreparedJob({
        output_folder: outputFolder,
        output_formats: studio.selectedFormats,
      });
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      toast.error(message);
    }
  };

  const handleCancel = async () => {
    try {
      await studio.cancelActiveJob();
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      toast.error(message);
    }
  };

  const handleRetry = async (jobId: string) => {
    try {
      await studio.retryJob(jobId);
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      toast.error(message);
    }
  };

  const handleOpenFolder = async (jobId: string) => {
    try {
      await studio.openOutputFolder(jobId);
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      toast.error(message);
    }
  };

  const handleDeleteJob = async (jobId: string) => {
    try {
      await studio.deleteJob(jobId);
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      toast.error(message);
    }
  };

  const handleClearAllJobs = async (jobIds: string[]) => {
    try {
      await studio.clearRecentJobs(jobIds);
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      toast.error(message);
    }
  };

  const handleSelectPreparedJob = async (jobId: string) => {
    try {
      await studio.selectRecentJob(jobId);
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      toast.error(message);
    }
  };

  const hasSeparateActiveJob =
    !!studio.activeJob &&
    (studio.activeJob.status === "running" ||
      studio.activeJob.status === "paused") &&
    studio.selectedJob !== null;

  const displayedJob = studio.selectedJob ?? studio.activeJob;
  const displayedJobIsActive =
    !!displayedJob && studio.activeJob?.id === displayedJob.id;
  const displayedJobStage = displayedJobIsActive
    ? studio.currentStage
    : displayedJob?.status === "done"
      ? "done"
      : displayedJob?.status === "error" || displayedJob?.status === "cancelled"
        ? "error"
        : displayedJob?.status === "paused"
          ? "paused"
          : displayedJob?.status === "running"
            ? "transcribing"
            : "idle";
  const displayedJobStatusMessage = displayedJobIsActive
    ? studio.statusMessage
    : null;
  const displayedJobPreparationProgress = displayedJobIsActive
    ? studio.preparationProgress
    : null;

  return (
    <div className="max-w-4xl w-full mx-auto space-y-5">
      <div className="flex items-start justify-between gap-3">
        <div className="space-y-2">
          <h1 className="text-2xl font-semibold">
            {t("studio.title", { defaultValue: "Studio" })}
          </h1>
          <p className="text-sm text-text/60">
            {t("studio.subtitle", {
              defaultValue:
                "Drop an audio file, choose output, start, wait, and get clean transcript files.",
            })}
          </p>
        </div>
        <Button
          type="button"
          variant="secondary"
          size="sm"
          className="px-2"
          onClick={() => void handleChooseAnotherFile()}
          disabled={
            !settings?.selected_model || studio.isPreparing || studio.isStarting
          }
          title={t("studio.dropzone.chooseFile", {
            defaultValue: "Choose File",
          })}
          aria-label={t("studio.dropzone.chooseFile", {
            defaultValue: "Choose File",
          })}
        >
          <UploadCloud className="h-4 w-4" />
        </Button>
      </div>
      {!settings?.selected_model && (
        <Alert variant="warning">
          {t("studio.selectModelWarning", {
            defaultValue:
              "Select a transcription model before starting Studio.",
          })}
        </Alert>
      )}

      {studio.error && studio.currentStage === "error" && (
        <Alert variant="error">{studio.error}</Alert>
      )}

      {hasSeparateActiveJob && (
        <div className="flex items-center justify-between gap-3 rounded-xl border border-blue-400/25 bg-blue-500/10 px-4 py-3 text-sm text-blue-200">
          <span>
            {t("studio.job.activeBrowsingNotice", {
              defaultValue:
                "Another Studio job is still running in the background.",
            })}
          </span>
          <button
            type="button"
            className="cursor-pointer rounded-lg border border-blue-300/30 px-3 py-1 text-xs font-medium text-blue-100 transition-colors hover:bg-blue-400/10"
            onClick={studio.returnToActiveJob}
          >
            {t("studio.job.returnToActive", {
              defaultValue: "Return to active job",
            })}
          </button>
        </div>
      )}

      {studio.preparedJob ? (
        <StudioSetupCard
          key={`${studio.preparedJob.id}-${studio.preparedJobSelectionToken}`}
          job={studio.preparedJob}
          outputFolder={outputFolder}
          selectedFormats={studio.selectedFormats}
          loadedFromRecent={studio.preparedJobOrigin === "recent"}
          onOutputFolderChange={(value) => {
            setOutputFolder(value);
            studio.setLastOutputFolder(value);
          }}
          onFormatsChange={studio.setSelectedFormats}
          onStart={handleStart}
          onCancel={studio.clearPreparedJob}
          disabled={studio.isStarting}
        />
      ) : displayedJob ? (
        <StudioJobView
          key={`${displayedJob.id}-${studio.preparedJobSelectionToken}`}
          job={displayedJob}
          statusMessage={displayedJobStatusMessage}
          stage={displayedJobStage}
          preparationProgress={displayedJobPreparationProgress}
          error={displayedJobIsActive ? studio.error : null}
          loadedFromRecent={studio.selectedJob?.id === displayedJob.id}
          onCancel={handleCancel}
          onRetry={() => handleRetry(displayedJob.id)}
          onOpenFolder={() => handleOpenFolder(displayedJob.id)}
        />
      ) : (
        <StudioDropzone
          supportedExtensions={studio.supportedExtensions}
          disabled={!settings?.selected_model || studio.isPreparing}
          onFileSelected={handlePrepare}
        />
      )}

      <StudioRecentList
        jobs={studio.recentJobs}
        onSelectJob={handleSelectPreparedJob}
        onOpenFolder={handleOpenFolder}
        onRetry={handleRetry}
        onDelete={handleDeleteJob}
        onClearAll={handleClearAllJobs}
        selectedJobId={studio.selectedRecentJobId}
        selectionToken={studio.preparedJobSelectionToken}
      />
    </div>
  );
};

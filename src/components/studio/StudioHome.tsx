import React from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { Alert } from "@/components/ui/Alert";
import { useSettings } from "@/hooks/useSettings";
import { useModelStore } from "@/stores/modelStore";
import { useStudioStore } from "@/stores/studioStore";
import { StudioDropzone } from "./StudioDropzone";
import { StudioJobView } from "./StudioJobView";
import { StudioRecentList } from "./StudioRecentList";
import { StudioSetupCard } from "./StudioSetupCard";

export const StudioHome: React.FC = () => {
  const { t } = useTranslation();
  const { settings } = useSettings();
  const studio = useStudioStore();
  const modelStore = useModelStore();

  React.useEffect(() => {
    studio.initialize().catch((error) => {
      console.error("Failed to initialize Studio:", error);
    });
  }, [studio]);

  React.useEffect(() => {
    modelStore.initialize().catch((error) => {
      console.error("Failed to initialize model store for Studio:", error);
    });
  }, [modelStore]);

  const [outputFolder, setOutputFolder] = React.useState(studio.lastOutputFolder);

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

  return (
    <div className="max-w-4xl w-full mx-auto space-y-5">
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
      {!settings?.selected_model && (
        <Alert variant="warning">
          {t("studio.selectModelWarning", {
            defaultValue: "Select a transcription model before starting Studio.",
          })}
        </Alert>
      )}

      {studio.error && studio.currentStage === "error" && (
        <Alert variant="error">{studio.error}</Alert>
      )}

      {studio.activeJob ? (
        <StudioJobView
          job={studio.activeJob}
          statusMessage={studio.statusMessage}
          stage={studio.currentStage}
          preparationProgress={studio.preparationProgress}
          error={studio.error}
          onCancel={handleCancel}
          onRetry={() => handleRetry(studio.activeJob!.id)}
          onOpenFolder={() => handleOpenFolder(studio.activeJob!.id)}
        />
      ) : studio.preparedJob ? (
        <StudioSetupCard
          job={studio.preparedJob}
          outputFolder={outputFolder}
          selectedFormats={studio.selectedFormats}
          onOutputFolderChange={(value) => {
            setOutputFolder(value);
            studio.setLastOutputFolder(value);
          }}
          onFormatsChange={studio.setSelectedFormats}
          onStart={handleStart}
          onCancel={studio.clearPreparedJob}
          disabled={studio.isStarting}
        />
      ) : (
        <StudioDropzone
          disabled={!settings?.selected_model || studio.isPreparing}
          onFileSelected={handlePrepare}
        />
      )}

      <StudioRecentList
        jobs={studio.recentJobs}
        onOpenFolder={handleOpenFolder}
        onRetry={handleRetry}
        onDelete={handleDeleteJob}
      />
    </div>
  );
};

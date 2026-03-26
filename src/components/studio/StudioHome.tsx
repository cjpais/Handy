import React from "react";
import { toast } from "sonner";
import { Alert } from "@/components/ui/Alert";
import { useSettings } from "@/hooks/useSettings";
import { useModelStore } from "@/stores/modelStore";
import { useStudioStore } from "@/stores/studioStore";
import { StudioDropzone } from "./StudioDropzone";
import { StudioJobView } from "./StudioJobView";
import { StudioRecentList } from "./StudioRecentList";
import { StudioSetupCard } from "./StudioSetupCard";
import { StudioStatusBar } from "./StudioStatusBar";

export const StudioHome: React.FC = () => {
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

  const selectedModel =
    modelStore.models.find((model) => model.id === settings?.selected_model) || null;
  const modelName = selectedModel?.name || settings?.selected_model || "";

  const [outputFolder, setOutputFolder] = React.useState(studio.lastOutputFolder);

  React.useEffect(() => {
    if (studio.preparedJob) {
      const fallbackFolder =
        studio.lastOutputFolder ||
        studio.preparedJob.output_folder ||
        studio.preparedJob.source_path.replace(/[\\/][^\\/]+$/, "");
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

  return (
    <div className="max-w-4xl w-full mx-auto space-y-5">
      <div className="space-y-2">
        <h1 className="text-2xl font-semibold">Studio</h1>
        <p className="text-sm text-text/60">
          Drop a file, choose output, start, wait, and get clean transcript files.
        </p>
      </div>

      <StudioStatusBar modelName={modelName} />

      {!settings?.selected_model && (
        <Alert variant="warning">
          Select a transcription model before starting Studio.
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
          error={studio.error}
          onCancel={studio.cancelActiveJob}
          onRetry={() => studio.retryJob(studio.activeJob!.id)}
          onOpenFolder={() => studio.openOutputFolder(studio.activeJob!.id)}
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
        onOpenFolder={studio.openOutputFolder}
        onRetry={studio.retryJob}
        onDelete={studio.deleteJob}
      />
    </div>
  );
};

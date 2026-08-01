import React, { useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { Check, Copy, FileArchive, FilePlus2, Loader2, X } from "lucide-react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { SettingsGroup, ToggleSwitch } from "@/components/ui";
import { useSettings } from "@/hooks/useSettings";
import {
  useFileTranscriptionStore,
  type FileJob,
} from "@/stores/fileTranscriptionStore";
import { Alert } from "../../ui/Alert";
import Badge from "../../ui/Badge";
import { Button } from "../../ui/Button";

const IconButton: React.FC<{
  onClick: () => void;
  title: string;
  disabled?: boolean;
  children: React.ReactNode;
}> = ({ onClick, title, disabled, children }) => (
  <button
    onClick={onClick}
    disabled={disabled}
    className="p-1.5 rounded-md flex items-center justify-center transition-colors cursor-pointer disabled:cursor-not-allowed disabled:text-text/20 text-text/50 hover:text-logo-primary"
    title={title}
  >
    {children}
  </button>
);

const AUDIO_EXTENSIONS = ["mp3", "wav", "flac", "ogg", "m4a", "aac"];

export const FileTranscriptionSettings: React.FC = () => {
  const { t } = useTranslation();
  const { settings } = useSettings();

  const jobs = useFileTranscriptionStore((state) => state.jobs);
  const running = useFileTranscriptionStore((state) => state.running);
  const postProcess = useFileTranscriptionStore((state) => state.postProcess);
  const addFiles = useFileTranscriptionStore((state) => state.addFiles);
  const removeFile = useFileTranscriptionStore((state) => state.removeFile);
  const clearAll = useFileTranscriptionStore((state) => state.clearAll);
  const setPostProcess = useFileTranscriptionStore(
    (state) => state.setPostProcess,
  );
  const start = useFileTranscriptionStore((state) => state.start);
  const cancel = useFileTranscriptionStore((state) => state.cancel);
  const exportZip = useFileTranscriptionStore((state) => state.exportZip);

  const [exporting, setExporting] = useState(false);

  const pendingCount = jobs.filter((job) => job.status !== "done").length;
  const completedCount = jobs.filter((job) => job.status === "done").length;

  const handleChooseFiles = async () => {
    try {
      const selected = await open({
        multiple: true,
        filters: [
          {
            name: t("settings.files.audioFilter"),
            extensions: AUDIO_EXTENSIONS,
          },
        ],
      });
      if (!selected) return;
      addFiles(Array.isArray(selected) ? selected : [selected]);
    } catch (error) {
      console.error("Failed to choose files:", error);
      toast.error(t("settings.files.chooseError"));
    }
  };

  const handleStart = async () => {
    try {
      await start();
    } catch (error) {
      console.error("Failed to start batch transcription:", error);
      toast.error(
        error instanceof Error ? error.message : t("settings.files.startError"),
      );
    }
  };

  const handleCancel = async () => {
    try {
      await cancel();
      toast.info(t("settings.files.cancelHint"));
    } catch (error) {
      console.error("Failed to cancel batch transcription:", error);
    }
  };

  const handleExport = async () => {
    try {
      setExporting(true);
      const saved = await exportZip();
      if (saved) {
        toast.success(t("settings.files.zipSaved"));
      }
    } catch (error) {
      console.error("Failed to export transcripts:", error);
      toast.error(
        error instanceof Error ? error.message : t("settings.files.zipError"),
      );
    } finally {
      setExporting(false);
    }
  };

  return (
    <div className="max-w-3xl w-full mx-auto space-y-4">
      <div className="mb-4">
        <h1 className="text-xl font-semibold mb-2">
          {t("settings.files.title")}
        </h1>
        <p className="text-sm text-text/60">
          {t("settings.files.description")}
        </p>
      </div>

      <div className="flex flex-wrap items-center gap-2">
        <Button
          onClick={handleChooseFiles}
          variant="primary"
          className="flex items-center gap-2"
          disabled={running}
        >
          <FilePlus2 className="w-4 h-4" />
          <span>{t("settings.files.chooseFiles")}</span>
        </Button>

        {running ? (
          <Button
            onClick={handleCancel}
            variant="secondary"
            className="flex items-center gap-2"
          >
            <X className="w-4 h-4" />
            <span>{t("settings.files.cancel")}</span>
          </Button>
        ) : (
          <Button
            onClick={handleStart}
            variant="primary-soft"
            className="flex items-center gap-2"
            disabled={pendingCount === 0}
          >
            <span>{t("settings.files.transcribe")}</span>
          </Button>
        )}

        <Button
          onClick={handleExport}
          variant="secondary"
          className="flex items-center gap-2"
          disabled={completedCount === 0 || running || exporting}
        >
          <FileArchive className="w-4 h-4" />
          <span>{t("settings.files.saveZip")}</span>
        </Button>

        {jobs.length > 0 && !running && (
          <Button onClick={clearAll} variant="ghost" size="sm">
            {t("settings.files.clearAll")}
          </Button>
        )}
      </div>

      {running && (
        <p className="text-xs text-text/50">
          {t("settings.files.shortcutsSuspended")}
        </p>
      )}

      {settings?.post_process_enabled && (
        <SettingsGroup>
          <ToggleSwitch
            checked={postProcess}
            onChange={setPostProcess}
            disabled={running}
            label={t("settings.files.postProcess")}
            description={t("settings.files.postProcessDescription")}
            grouped
          />
        </SettingsGroup>
      )}

      <div className="bg-background border border-mid-gray/20 rounded-lg overflow-visible">
        {jobs.length === 0 ? (
          <div className="text-center py-8 text-text/50">
            {t("settings.files.empty")}
          </div>
        ) : (
          <div className="divide-y divide-mid-gray/20">
            {jobs.map((job) => (
              <FileJobRow
                key={job.path}
                job={job}
                running={running}
                onRemove={() => removeFile(job.path)}
              />
            ))}
          </div>
        )}
      </div>
    </div>
  );
};

interface FileJobRowProps {
  job: FileJob;
  running: boolean;
  onRemove: () => void;
}

const FileJobRow: React.FC<FileJobRowProps> = ({ job, running, onRemove }) => {
  const { t } = useTranslation();
  const [showCopied, setShowCopied] = useState(false);

  const handleCopy = async () => {
    if (!job.text) return;
    try {
      await navigator.clipboard.writeText(job.text);
      setShowCopied(true);
      setTimeout(() => setShowCopied(false), 2000);
    } catch (error) {
      console.error("Failed to copy to clipboard:", error);
    }
  };

  let badge: React.ReactNode;
  if (job.status === "transcribing") {
    badge = (
      <Badge variant="primary" className="gap-1.5">
        <Loader2 className="w-3 h-3 animate-spin" />
        {t("settings.files.status.transcribing")}
      </Badge>
    );
  } else if (job.status === "done") {
    badge = <Badge variant="success">{t("settings.files.status.done")}</Badge>;
  } else if (job.status === "failed") {
    badge = (
      <Badge variant="secondary">{t("settings.files.status.failed")}</Badge>
    );
  } else {
    badge = (
      <Badge variant="secondary">{t("settings.files.status.queued")}</Badge>
    );
  }

  return (
    <div className="px-4 py-2 pb-5 flex flex-col gap-3">
      <div className="flex justify-between items-center gap-2">
        <p className="text-sm font-medium truncate" title={job.path}>
          {job.name}
        </p>
        <div className="flex items-center gap-2 shrink-0">
          {badge}
          {job.status === "done" && (
            <IconButton
              onClick={handleCopy}
              title={t("settings.files.copyToClipboard")}
            >
              {showCopied ? (
                <Check width={16} height={16} />
              ) : (
                <Copy width={16} height={16} />
              )}
            </IconButton>
          )}
          {!running && (
            <IconButton onClick={onRemove} title={t("settings.files.remove")}>
              <X width={16} height={16} />
            </IconButton>
          )}
        </div>
      </div>

      {job.status === "done" && job.text && (
        <p className="text-sm text-text/90 select-text cursor-text whitespace-pre-wrap break-words">
          {job.text}
        </p>
      )}

      {job.status === "failed" && job.error && (
        <Alert variant="error" contained>
          {job.error}
        </Alert>
      )}
    </div>
  );
};

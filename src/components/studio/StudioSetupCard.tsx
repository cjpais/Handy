import React from "react";
import { useTranslation } from "react-i18next";
import { open } from "@tauri-apps/plugin-dialog";
import { FolderOpen } from "lucide-react";
import { Button } from "@/components/ui/Button";
import { Alert } from "@/components/ui/Alert";
import type { StudioFormat, StudioJob } from "@/lib/types/studio";

const FORMATS: StudioFormat[] = ["txt", "srt", "vtt"];

interface StudioSetupCardProps {
  job: StudioJob;
  outputFolder: string;
  selectedFormats: StudioFormat[];
  onOutputFolderChange: (value: string) => void;
  onFormatsChange: (value: StudioFormat[]) => void;
  onStart: () => Promise<void>;
  onCancel: () => void;
  disabled?: boolean;
}

const formatBytes = (bytes: number) => {
  if (!bytes) return "Unknown size";
  const gb = bytes / 1024 / 1024 / 1024;
  if (gb >= 1) return `${gb.toFixed(1)} GB`;
  const mb = bytes / 1024 / 1024;
  return `${mb.toFixed(0)} MB`;
};

const formatDuration = (durationMs: number) => {
  const totalSeconds = Math.round(durationMs / 1000);
  const minutes = Math.floor(totalSeconds / 60);
  const seconds = totalSeconds % 60;
  return `${minutes}m ${seconds}s`;
};

export const StudioSetupCard: React.FC<StudioSetupCardProps> = ({
  job,
  outputFolder,
  selectedFormats,
  onOutputFolderChange,
  onFormatsChange,
  onStart,
  onCancel,
  disabled = false,
}) => {
  const { t } = useTranslation();

  const chooseFolder = async () => {
    const selected = await open({
      directory: true,
      multiple: false,
    });
    if (typeof selected === "string") {
      onOutputFolderChange(selected);
    }
  };

  const toggleFormat = (format: StudioFormat) => {
    if (selectedFormats.includes(format)) {
      onFormatsChange(selectedFormats.filter((item) => item !== format));
      return;
    }
    onFormatsChange([...selectedFormats, format]);
  };

  return (
    <div className="rounded-2xl border border-mid-gray/20 bg-background p-5">
      <div className="flex flex-col gap-5">
        <div>
          <h2 className="text-lg font-semibold">{job.source_name}</h2>
          <div className="mt-3 grid gap-2 text-sm text-text/65 sm:grid-cols-2">
            <p>
              {t("studio.setup.duration", {
                defaultValue: "Duration: {{value}}",
                value: formatDuration(job.media_duration_ms),
              })}
            </p>
            <p>
              {t("studio.setup.size", {
                defaultValue: "Size: {{value}}",
                value: formatBytes(job.file_size_bytes),
              })}
            </p>
            <p>
              {t("studio.setup.format", {
                defaultValue: "Format: {{format}}",
                format:
                  job.container_format ||
                  t("studio.common.unknown", { defaultValue: "Unknown" }),
              })}
              {job.audio_codec ? ` - ${job.audio_codec}` : ""}
            </p>
            <p>
              {t("studio.setup.estimate", {
                defaultValue: "Estimate: {{value}}",
                value:
                  job.estimate_text ||
                  t("studio.common.estimateFallback", {
                    defaultValue: "About a few minutes",
                  }),
              })}
            </p>
          </div>
        </div>

        <div className="space-y-3">
          <div>
            <p className="text-sm font-medium">
              {t("studio.setup.outputFolder", { defaultValue: "Output folder" })}
            </p>
            <div className="mt-2 flex gap-2">
              <div className="flex-1 rounded-lg border border-mid-gray/20 bg-mid-gray/5 px-3 py-2 text-sm text-text/70">
                {outputFolder ||
                  t("studio.setup.chooseOutput", {
                    defaultValue: "Choose where to save your transcript",
                  })}
              </div>
              <Button variant="secondary" onClick={chooseFolder} disabled={disabled}>
                <FolderOpen className="h-4 w-4" />
              </Button>
            </div>
          </div>

          <div>
            <p className="text-sm font-medium">
              {t("studio.setup.formats", { defaultValue: "Formats" })}
            </p>
            <div className="mt-2 flex flex-wrap gap-2">
              {FORMATS.map((format) => {
                const active = selectedFormats.includes(format);
                return (
                  <Button
                    key={format}
                    variant={active ? "primary-soft" : "secondary"}
                    size="sm"
                    onClick={() => toggleFormat(format)}
                    disabled={disabled}
                  >
                    {format.toUpperCase()}
                  </Button>
                );
              })}
            </div>
          </div>
        </div>

        {selectedFormats.length === 0 && (
          <Alert variant="warning">
            {t("studio.setup.selectFormatWarning", {
              defaultValue: "Select at least one output format.",
            })}
          </Alert>
        )}

        <div className="flex flex-wrap gap-3">
          <Button
            onClick={onStart}
            disabled={disabled || !outputFolder || selectedFormats.length === 0}
          >
            {t("studio.setup.start", { defaultValue: "Start" })}
          </Button>
          <Button variant="secondary" onClick={onCancel} disabled={disabled}>
            {t("studio.setup.chooseAnotherFile", {
              defaultValue: "Choose another file",
            })}
          </Button>
        </div>
      </div>
    </div>
  );
};

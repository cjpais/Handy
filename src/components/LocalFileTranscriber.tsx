import React, { useState } from "react";
import { useTranslation } from "react-i18next";
import { X, FileAudio, FilePlus2, Play, Trash2 } from "lucide-react";
import { toast } from "sonner";
import { open } from "@tauri-apps/plugin-dialog";
import { Button } from "./ui/Button";
import { commands } from "@/bindings";

interface LocalFileTranscriberProps {
  initialFiles: string[];
  onClose: () => void;
  onSuccess: (action: string) => void;
}

export const LocalFileTranscriber: React.FC<LocalFileTranscriberProps> = ({
  initialFiles,
  onClose,
  onSuccess,
}) => {
  const { t } = useTranslation();
  const [files, setFiles] = useState<string[]>(initialFiles);
  const [action, setAction] = useState<"meeting" | "transcribe">("meeting");

  const handleAddMore = async () => {
    try {
      const selected = await open({
        multiple: true,
        filters: [
          {
            name: "Audio",
            extensions: ["wav", "mp3", "m4a", "flac", "ogg"],
          },
        ],
      });
      if (selected) {
        const newFiles = Array.isArray(selected) ? selected : [selected];
        setFiles((prev) => [
          ...prev,
          ...newFiles.filter((f) => !prev.includes(f)),
        ]);
      }
    } catch (error) {
      console.error("Failed to open file dialog:", error);
    }
  };

  const removeFile = (fileToRemove: string) => {
    setFiles((prev) => prev.filter((f) => f !== fileToRemove));
  };

  const handleTranscribe = () => {
    if (files.length === 0) return;

    const filesToProcess = [...files];
    const targetAction = action;

    // Close dialog immediately
    onClose();

    // Detach and process in background
    (async () => {
      let successCount = 0;
      toast.info(
        `Started transcription for ${filesToProcess.length} file(s) in background.`,
      );
      for (let i = 0; i < filesToProcess.length; i++) {
        try {
          const result = await commands.processLocalFile(
            filesToProcess[i],
            targetAction,
          );
          if (result.status === "ok") {
            successCount++;
          } else {
            toast.error(
              `Failed to process ${filesToProcess[i].split(/[/\\]/).pop()}: ${result.error}`,
            );
          }
        } catch (error: any) {
          toast.error(
            `Error processing ${filesToProcess[i].split(/[/\\]/).pop()}: ${error.message || error}`,
          );
        }
      }

      if (successCount > 0) {
        toast.success(`Successfully processed ${successCount} file(s)`);
        onSuccess(targetAction);
      }
    })();
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-[#0a0908]/80 backdrop-blur-sm">
      <div className="bg-orange-off-white border border-stone-mist rounded-[16px] shadow-xl w-full max-w-md max-h-[80vh] flex flex-col overflow-hidden">
        <div className="flex items-center justify-between p-4 border-b border-stone-mist">
          <h2 className="text-md font-bold font-cooper flex items-center gap-2 text-charcoal">
            <FileAudio className="w-5 h-5 text-forest-green" />
            {t("localFileTranscriber.title")}
          </h2>
          <button
            onClick={onClose}
            className="p-1 rounded-md hover:bg-stone-mist/30 text-bark-grey transition-colors cursor-pointer"
          >
            <X className="w-5 h-5" />
          </button>
        </div>

        <div className="p-4 flex-1 overflow-y-auto space-y-4">
          {files.length === 0 ? (
            <div className="text-center text-bark-grey py-8 font-mono text-xs uppercase tracking-wider">
              {t("localFileTranscriber.empty")}
            </div>
          ) : (
            <div className="space-y-2">
              <div className="text-xs font-semibold uppercase tracking-[0.04em] font-mono text-bark-grey mb-2">
                {t("localFileTranscriber.selectedFiles")}
              </div>
              {files.map((file, idx) => {
                const fileName = file.split(/[/\\]/).pop() || file;
                return (
                  <div
                    key={idx}
                    className="flex items-center justify-between bg-warm-bone/45 border border-stone-mist rounded-[8px] p-2 px-3 transition-colors hover:bg-warm-bone/80"
                  >
                    <span className="text-sm text-charcoal truncate mr-4 flex-1" title={file}>
                      {fileName}
                    </span>
                    <button
                      onClick={() => removeFile(file)}
                      className="text-bark-grey hover:text-alarm-red transition-colors cursor-pointer"
                    >
                      <Trash2 className="w-4 h-4" />
                    </button>
                  </div>
                );
              })}
            </div>
          )}

          <div className="pt-2 border-t border-stone-mist">
            <button
              onClick={handleAddMore}
              className="flex items-center gap-2 text-xs font-semibold uppercase tracking-[0.04em] font-mono text-forest-green hover:text-deep-forest-green transition-colors cursor-pointer"
            >
              <FilePlus2 className="w-4 h-4" />
              {t("localFileTranscriber.addMoreFiles")}
            </button>
          </div>

          <div className="space-y-3 pt-4 border-t border-stone-mist">
            <div className="text-xs font-semibold uppercase tracking-[0.04em] font-mono text-bark-grey">
              {t("localFileTranscriber.action")}
            </div>
            <div className="flex flex-col gap-2">
              <label className="flex items-center gap-3 p-3 rounded-[12px] border cursor-pointer transition-colors bg-warm-bone/45 border-stone-mist hover:border-forest-green/50">
                <input
                  type="radio"
                  name="action"
                  value="meeting"
                  checked={action === "meeting"}
                  onChange={() => setAction("meeting")}
                  className="w-4 h-4 text-forest-green focus:ring-forest-green bg-orange-off-white border-stone-mist"
                />
                <div className="flex flex-col">
                  <span className="text-sm font-semibold text-charcoal">
                    {t("localFileTranscriber.summarizeAsMeeting")}
                  </span>
                  <span className="text-xs text-bark-grey">
                    {t("localFileTranscriber.summarizeAsMeetingDesc")}
                  </span>
                </div>
              </label>

              <label className="flex items-center gap-3 p-3 rounded-[12px] border cursor-pointer transition-colors bg-warm-bone/45 border-stone-mist hover:border-forest-green/50">
                <input
                  type="radio"
                  name="action"
                  value="transcribe"
                  checked={action === "transcribe"}
                  onChange={() => setAction("transcribe")}
                  className="w-4 h-4 text-forest-green focus:ring-forest-green bg-orange-off-white border-stone-mist"
                />
                <div className="flex flex-col">
                  <span className="text-sm font-semibold text-charcoal">
                    {t("localFileTranscriber.plainTranscribe")}
                  </span>
                  <span className="text-xs text-bark-grey">
                    {t("localFileTranscriber.plainTranscribeDesc")}
                  </span>
                </div>
              </label>
            </div>
          </div>
        </div>

        <div className="p-4 border-t border-stone-mist bg-orange-off-white/80 flex flex-col gap-3">
          <div className="flex justify-end gap-3">
            <Button variant="ghost" onClick={onClose}>
              {t("common.cancel")}
            </Button>
            <Button
              variant="primary"
              onClick={handleTranscribe}
              disabled={files.length === 0}
              className="flex items-center gap-2"
            >
              <Play className="w-3.5 h-3.5" />
              {t("localFileTranscriber.startTranscription")}
            </Button>
          </div>
        </div>
      </div>
    </div>
  );
};

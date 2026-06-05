import React, { useState } from "react";
import { useTranslation } from "react-i18next";
import { X, FileAudio, FilePlus2, Play, Trash2 } from "lucide-react";
import { toast } from "sonner";
import { open } from "@tauri-apps/plugin-dialog";
import { Button } from "./ui/Button";
import { commands, events } from "@/bindings";

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
  const [isProcessing, setIsProcessing] = useState(false);
  const [progress, setProgress] = useState({ current: 0, total: 0 });
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
        setFiles((prev) => [...prev, ...newFiles.filter((f) => !prev.includes(f))]);
      }
    } catch (error) {
      console.error("Failed to open file dialog:", error);
    }
  };

  const removeFile = (fileToRemove: string) => {
    setFiles((prev) => prev.filter((f) => f !== fileToRemove));
  };

  const handleTranscribe = async () => {
    if (files.length === 0) return;
    setIsProcessing(true);
    setProgress({ current: 0, total: files.length });

    let successCount = 0;
    for (let i = 0; i < files.length; i++) {
      setProgress({ current: i + 1, total: files.length });
      try {
        const result = await commands.processLocalFile(files[i], action);
        if (result.status === "ok") {
          successCount++;
        } else {
          toast.error(`Failed to process ${files[i].split(/[/\\]/).pop()}: ${result.error}`);
        }
      } catch (error: any) {
        toast.error(`Error processing ${files[i].split(/[/\\]/).pop()}: ${error.message || error}`);
      }
    }

    setIsProcessing(false);
    if (successCount > 0) {
      toast.success(`Successfully processed ${successCount} file(s)`);
      onSuccess(action);
    }
    onClose();
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-background/80 backdrop-blur-sm">
      <div className="bg-background border border-mid-gray/20 rounded-xl shadow-2xl w-full max-w-md max-h-[80vh] flex flex-col overflow-hidden">
        <div className="flex items-center justify-between p-4 border-b border-mid-gray/20">
          <h2 className="text-lg font-semibold flex items-center gap-2">
            <FileAudio className="w-5 h-5 text-logo-primary" />
            Transcribe Local Files
          </h2>
          <button
            onClick={onClose}
            disabled={isProcessing}
            className="p-1 rounded-md hover:bg-mid-gray/10 text-mid-gray transition-colors disabled:opacity-50"
          >
            <X className="w-5 h-5" />
          </button>
        </div>

        <div className="p-4 flex-1 overflow-y-auto space-y-4">
          {files.length === 0 ? (
            <div className="text-center text-mid-gray py-8">
              No files selected. Add some audio files to transcribe.
            </div>
          ) : (
            <div className="space-y-2">
              <div className="text-sm font-medium text-text/80 mb-2">Selected Files:</div>
              {files.map((file, idx) => {
                const fileName = file.split(/[/\\]/).pop() || file;
                return (
                  <div
                    key={idx}
                    className="flex items-center justify-between bg-mid-gray/5 border border-mid-gray/10 rounded-lg p-2 px-3"
                  >
                    <span className="text-sm truncate mr-4 flex-1" title={file}>
                      {fileName}
                    </span>
                    <button
                      onClick={() => removeFile(file)}
                      disabled={isProcessing}
                      className="text-mid-gray hover:text-red-500 transition-colors disabled:opacity-50"
                    >
                      <Trash2 className="w-4 h-4" />
                    </button>
                  </div>
                );
              })}
            </div>
          )}

          <div className="pt-2 border-t border-mid-gray/10">
            <button
              onClick={handleAddMore}
              disabled={isProcessing}
              className="flex items-center gap-2 text-sm text-logo-primary hover:text-logo-primary/80 font-medium transition-colors disabled:opacity-50"
            >
              <FilePlus2 className="w-4 h-4" />
              Add more files
            </button>
          </div>

          <div className="space-y-3 pt-4 border-t border-mid-gray/10">
            <div className="text-sm font-medium text-text/80">Action:</div>
            <div className="flex flex-col gap-2">
              <label className="flex items-center gap-3 p-3 rounded-lg border cursor-pointer transition-colors bg-mid-gray/5 border-mid-gray/20 hover:border-logo-primary/50">
                <input
                  type="radio"
                  name="action"
                  value="meeting"
                  checked={action === "meeting"}
                  onChange={() => setAction("meeting")}
                  disabled={isProcessing}
                  className="w-4 h-4 text-logo-primary focus:ring-logo-primary bg-background border-mid-gray/30"
                />
                <div className="flex flex-col">
                  <span className="text-sm font-medium">Summarize as Meeting</span>
                  <span className="text-xs text-mid-gray">Transcribes and summarizes to English (Meetings tab)</span>
                </div>
              </label>

              <label className="flex items-center gap-3 p-3 rounded-lg border cursor-pointer transition-colors bg-mid-gray/5 border-mid-gray/20 hover:border-logo-primary/50">
                <input
                  type="radio"
                  name="action"
                  value="transcribe"
                  checked={action === "transcribe"}
                  onChange={() => setAction("transcribe")}
                  disabled={isProcessing}
                  className="w-4 h-4 text-logo-primary focus:ring-logo-primary bg-background border-mid-gray/30"
                />
                <div className="flex flex-col">
                  <span className="text-sm font-medium">Plain Transcribe</span>
                  <span className="text-xs text-mid-gray">Basic Malayalam transcription (History tab)</span>
                </div>
              </label>
            </div>
          </div>
        </div>

        <div className="p-4 border-t border-mid-gray/20 bg-mid-gray/5 flex flex-col gap-3">
          {isProcessing && (
            <div className="text-sm text-center text-logo-primary font-medium animate-pulse">
              Processing {progress.current} of {progress.total}...
            </div>
          )}
          <div className="flex justify-end gap-3">
            <Button
              variant="ghost"
              onClick={onClose}
              disabled={isProcessing}
            >
              Cancel
            </Button>
            <Button
              variant="primary"
              onClick={handleTranscribe}
              disabled={isProcessing || files.length === 0}
              className="flex items-center gap-2"
            >
              <Play className="w-4 h-4" />
              Start Transcription
            </Button>
          </div>
        </div>
      </div>
    </div>
  );
};

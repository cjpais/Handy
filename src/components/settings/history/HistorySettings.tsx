import React, { useState, useEffect, useCallback } from "react";
import { AudioPlayer } from "../../ui/AudioPlayer";
import { Button } from "../../ui/Button";
import {
  Copy,
  Star,
  Check,
  Trash2,
  FolderOpen,
  Upload,
  Loader2,
  Mic,
  FileText,
} from "lucide-react";
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import { toast } from "sonner";
import { sendNotification } from "@tauri-apps/plugin-notification";

interface HistoryEntry {
  id: number;
  file_name: string;
  timestamp: number;
  saved: boolean;
  title: string;
  transcription_text: string;
  duration?: number;
  source?: string;
}

interface OpenRecordingsButtonProps {
  onClick: () => void;
}

const OpenRecordingsButton: React.FC<OpenRecordingsButtonProps> = ({
  onClick,
}) => (
  <Button
    onClick={onClick}
    variant="secondary"
    size="sm"
    className="flex items-center gap-2"
    title="Open recordings folder"
  >
    <FolderOpen className="w-4 h-4" />
    <span>Open Recordings Folder</span>
  </Button>
);

export const HistorySettings: React.FC = () => {
  const [historyEntries, setHistoryEntries] = useState<HistoryEntry[]>([]);
  const [loading, setLoading] = useState(true);
  const [isImporting, setIsImporting] = useState(false);
  const [importStatus, setImportStatus] = useState<string>("");
  const [progress, setProgress] = useState<number>(0);

  const loadHistoryEntries = useCallback(async () => {
    try {
      const entries = await invoke<HistoryEntry[]>("get_history_entries");
      setHistoryEntries(entries);
    } catch (error) {
      console.error("Failed to load history entries:", error);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    loadHistoryEntries();

    // Listen for history update events
    const setupListener = async () => {
      const unlistenHistory = await listen("history-updated", () => {
        console.log("History updated, reloading entries...");
        loadHistoryEntries();
      });

      const unlistenImport = await listen<string>("import-status", (event) => {
        setImportStatus(event.payload);
        if (event.payload === "Completed") {
          sendNotification({
            title: "Import Successful",
            body: "Audio file has been imported and transcribed.",
          });
        } else if (event.payload === "Failed") {
          sendNotification({
            title: "Import Failed",
            body: "Check the app for details.",
          });
        }
      });

      const unlistenProgress = await listen<number>(
        "transcription-progress",
        (event) => {
          setProgress(event.payload);
        },
      );

      // Return cleanup function
      return () => {
        unlistenHistory();
        unlistenImport();
        unlistenProgress();
      };
    };

    let unlistenPromise = setupListener();

    return () => {
      unlistenPromise.then((unlisten) => {
        if (unlisten) {
          unlisten();
        }
      });
    };
  }, [loadHistoryEntries]);

  const toggleSaved = async (id: number) => {
    try {
      await invoke("toggle_history_entry_saved", { id });
      // No need to reload here - the event listener will handle it
    } catch (error) {
      console.error("Failed to toggle saved status:", error);
    }
  };

  const copyToClipboard = async (text: string) => {
    try {
      await navigator.clipboard.writeText(text);
      toast.success("Transcription copied to clipboard");
    } catch (error) {
      console.error("Failed to copy to clipboard:", error);
      toast.error("Failed to copy to clipboard");
    }
  };

  const getAudioUrl = async (fileName: string) => {
    try {
      const filePath = await invoke<string>("get_audio_file_path", {
        fileName,
      });

      return convertFileSrc(`${filePath}`, "asset");
    } catch (error) {
      console.error("Failed to get audio file path:", error);
      return null;
    }
  };

  const deleteAudioEntry = async (id: number) => {
    try {
      await invoke("delete_history_entry", { id });
      toast.success("Entry deleted");
    } catch (error) {
      console.error("Failed to delete audio entry:", error);
      toast.error("Failed to delete entry");
      throw error;
    }
  };

  const openRecordingsFolder = async () => {
    try {
      await invoke("open_recordings_folder");
    } catch (error) {
      console.error("Failed to open recordings folder:", error);
    }
  };

  const handleImport = async () => {
    try {
      const selected = await open({
        multiple: false,
        filters: [
          {
            name: "Audio",
            extensions: ["mp3", "m4a", "wav"],
          },
        ],
      });

      if (selected) {
        setIsImporting(true);
        setImportStatus("Initializing...");
        setProgress(0);
        toast.info("Importing and transcribing audio...");

        await invoke("import_audio_file", { filePath: selected });

        toast.success("Audio imported successfully");
        // Refresh is handled by the event listener
      }
    } catch (error) {
      console.error("Failed to import audio:", error);
      toast.error(`Import failed: ${error}`);
    } finally {
      setIsImporting(false);
      setImportStatus("");
      setProgress(0);
    }
  };

  if (loading) {
    return (
      <div className="max-w-3xl w-full mx-auto space-y-6">
        <div className="space-y-2">
          <div className="px-4 flex items-center justify-between">
            <div>
              <h2 className="text-xs font-medium text-mid-gray uppercase tracking-wide">
                History
              </h2>
            </div>
            <div className="flex gap-2">
              <Button
                onClick={handleImport}
                variant="secondary"
                size="sm"
                className="flex items-center gap-2"
                disabled={isImporting}
              >
                {isImporting ? (
                  <Loader2 className="w-4 h-4 animate-spin" />
                ) : (
                  <Upload className="w-4 h-4" />
                )}
                <span>
                  {isImporting
                    ? importStatus || "Transcribing..."
                    : "Import Audio File"}
                </span>
              </Button>
              <OpenRecordingsButton onClick={openRecordingsFolder} />
            </div>
          </div>
          <div className="bg-background border border-mid-gray/20 rounded-lg overflow-visible">
            <div className="px-4 py-3 text-center text-text/60">
              Loading history...
            </div>
          </div>
        </div>
      </div>
    );
  }

  if (historyEntries.length === 0) {
    return (
      <div className="max-w-3xl w-full mx-auto space-y-6">
        <div className="space-y-2">
          <div className="px-4 flex items-center justify-between">
            <div>
              <h2 className="text-xs font-medium text-mid-gray uppercase tracking-wide">
                History
              </h2>
            </div>
            <div className="flex gap-2">
              <Button
                onClick={handleImport}
                variant="secondary"
                size="sm"
                className="flex items-center gap-2"
                disabled={isImporting}
              >
                {isImporting ? (
                  <Loader2 className="w-4 h-4 animate-spin" />
                ) : (
                  <Upload className="w-4 h-4" />
                )}
                <span>
                  {isImporting
                    ? importStatus || "Transcribing..."
                    : "Import Audio File"}
                </span>
              </Button>
              <OpenRecordingsButton onClick={openRecordingsFolder} />
            </div>
          </div>
          <div className="bg-background border border-mid-gray/20 rounded-lg overflow-visible">
            <div className="px-4 py-3 text-center text-text/60">
              No transcriptions yet. Start recording to build your history!
            </div>
          </div>
        </div>
      </div>
    );
  }

  return (
    <div className="max-w-3xl w-full mx-auto space-y-6">
      <div className="space-y-2">
        <div className="px-4 flex items-center justify-between">
          <div>
            <h2 className="text-xs font-medium text-mid-gray uppercase tracking-wide">
              History
            </h2>
          </div>
          <div className="flex gap-2">
            <Button
              onClick={handleImport}
              variant="secondary"
              size="sm"
              className="flex items-center gap-2"
              disabled={isImporting}
            >
              {isImporting ? (
                <Loader2 className="w-4 h-4 animate-spin" />
              ) : (
                <Upload className="w-4 h-4" />
              )}
              <span>
                {isImporting
                  ? `${importStatus || "Transcribing..."} ${progress > 0 ? `(${Math.round(progress)}%)` : ""}`
                  : "Import Audio File"}
              </span>
            </Button>
            <OpenRecordingsButton onClick={openRecordingsFolder} />
          </div>
        </div>
        {isImporting && (
          <div className="px-4 -mt-1">
            <div className="w-full h-1 bg-mid-gray/10 rounded-full overflow-hidden">
              <div
                className="h-full bg-logo-primary transition-all duration-300 ease-out"
                style={{ width: `${progress}%` }}
              />
            </div>
          </div>
        )}
        <div className="bg-background border border-mid-gray/20 rounded-lg overflow-visible">
          <div className="divide-y divide-mid-gray/20">
            {historyEntries.map((entry) => (
              <HistoryEntryComponent
                key={entry.id}
                entry={entry}
                onToggleSaved={() => toggleSaved(entry.id)}
                onCopyText={() => copyToClipboard(entry.transcription_text)}
                getAudioUrl={getAudioUrl}
                deleteAudio={deleteAudioEntry}
              />
            ))}
          </div>
        </div>
      </div>
    </div>
  );
};

interface HistoryEntryProps {
  entry: HistoryEntry;
  onToggleSaved: () => void;
  onCopyText: () => void;
  getAudioUrl: (fileName: string) => Promise<string | null>;
  deleteAudio: (id: number) => Promise<void>;
}

const HistoryEntryComponent: React.FC<HistoryEntryProps> = ({
  entry,
  onToggleSaved,
  onCopyText,
  getAudioUrl,
  deleteAudio,
}) => {
  const [audioUrl, setAudioUrl] = useState<string | null>(null);
  const [showCopied, setShowCopied] = useState(false);

  useEffect(() => {
    const loadAudio = async () => {
      const url = await getAudioUrl(entry.file_name);
      setAudioUrl(url);
    };
    loadAudio();
  }, [entry.file_name, getAudioUrl]);

  const handleCopyText = () => {
    onCopyText();
    setShowCopied(true);
    setTimeout(() => setShowCopied(false), 2000);
  };

  const handleDeleteEntry = async () => {
    try {
      await deleteAudio(entry.id);
    } catch (error) {
      console.error("Failed to delete entry:", error);
      toast.error("Failed to delete entry");
    }
  };

  return (
    <div className="px-4 py-2 pb-5 flex flex-col gap-3">
      <div className="flex justify-between items-center">
        <div className="flex items-center gap-2">
          {entry.source === "upload" ? (
            <span title="Uploaded File">
              <FileText className="w-4 h-4 text-mid-gray" />
            </span>
          ) : (
            <span title="Recording">
              <Mic className="w-4 h-4 text-mid-gray" />
            </span>
          )}
          <p className="text-sm font-medium">{entry.title}</p>
          {entry.duration && (
            <span className="text-xs text-mid-gray bg-mid-gray/10 px-1.5 py-0.5 rounded">
              {formatDuration(entry.duration)}
            </span>
          )}
        </div>
        <div className="flex items-center gap-1">
          <button
            onClick={handleCopyText}
            className="text-text/50 hover:text-logo-primary  hover:border-logo-primary transition-colors cursor-pointer"
            title="Copy transcription to clipboard"
          >
            {showCopied ? (
              <Check width={16} height={16} />
            ) : (
              <Copy width={16} height={16} />
            )}
          </button>
          <button
            onClick={onToggleSaved}
            className={`p-2 rounded  transition-colors cursor-pointer ${
              entry.saved
                ? "text-logo-primary hover:text-logo-primary/80"
                : "text-text/50 hover:text-logo-primary"
            }`}
            title={entry.saved ? "Remove from saved" : "Save transcription"}
          >
            <Star
              width={16}
              height={16}
              fill={entry.saved ? "currentColor" : "none"}
            />
          </button>
          <button
            onClick={handleDeleteEntry}
            className="text-text/50 hover:text-logo-primary transition-colors cursor-pointer"
            title="Delete entry"
          >
            <Trash2 width={16} height={16} />
          </button>
        </div>
      </div>
      <p className="italic text-text/90 text-sm pb-2">
        {entry.transcription_text}
      </p>
      {audioUrl && <AudioPlayer src={audioUrl} className="w-full" />}
    </div>
  );
};

function formatDuration(seconds: number): string {
  const minutes = Math.floor(seconds / 60);
  const remainingSeconds = Math.floor(seconds % 60);
  return `${minutes}:${remainingSeconds.toString().padStart(2, "0")}`;
}

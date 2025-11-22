import React, { useState, useEffect, useCallback } from "react";
import { AudioPlayer } from "../../ui/AudioPlayer";
import { Button } from "../../ui/Button";
import { Copy, Star, Check, Trash2, FolderOpen, Upload, Loader2, AlertTriangle, Mic, FileText } from "lucide-react";
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";

interface HistoryEntry {
  id: number;
  file_name: string;
  timestamp: number;
  saved: boolean;
  title: string;
  transcription_text: string;
  source_file_path?: string;
}

interface AudioFileStatus {
  path: string;
  exists: boolean;
  is_uploaded: boolean;
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
  const [isTranscribing, setIsTranscribing] = useState(false);
  const [transcriptionError, setTranscriptionError] = useState<string | null>(null);

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
      const historyUnlisten = await listen("history-updated", () => {
        console.log("History updated, reloading entries...");
        loadHistoryEntries();
      });

      const startedUnlisten = await listen("file-transcription-started", () => {
        console.log("File transcription started");
        setIsTranscribing(true);
        setTranscriptionError(null);
      });

      const completedUnlisten = await listen("file-transcription-completed", () => {
        console.log("File transcription completed");
        setIsTranscribing(false);
        setTranscriptionError(null);
        // History will be reloaded via history-updated event
      });

      const failedUnlisten = await listen<{ error: string }>("file-transcription-failed", (event) => {
        console.error("File transcription failed:", event.payload.error);
        setIsTranscribing(false);
        setTranscriptionError(event.payload.error);
        // Clear error after 5 seconds
        setTimeout(() => setTranscriptionError(null), 5000);
      });

      // Return cleanup function
      return () => {
        historyUnlisten();
        startedUnlisten();
        completedUnlisten();
        failedUnlisten();
      };
    };

    let unlistenPromise = setupListener();

    return () => {
      unlistenPromise.then((cleanup) => {
        if (cleanup) {
          cleanup();
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
    } catch (error) {
      console.error("Failed to copy to clipboard:", error);
    }
  };

  const getAudioUrl = async (entryId: number): Promise<AudioFileStatus | null> => {
    try {
      const fileStatus = await invoke<AudioFileStatus>("get_audio_file_path_for_entry", {
        id: entryId,
      });

      const url = convertFileSrc(fileStatus.path, "asset");
      return {
        path: url,
        exists: fileStatus.exists,
        is_uploaded: fileStatus.is_uploaded,
      };
    } catch (error) {
      console.error("Failed to get audio file path:", error);
      return null;
    }
  };

  const deleteAudioEntry = async (id: number) => {
    try {
      await invoke("delete_history_entry", { id });
    } catch (error) {
      console.error("Failed to delete audio entry:", error);
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

  const handleUploadAudio = async () => {
    setTranscriptionError(null);
    try {
      const selected = await open({
        multiple: false,
        filters: [
          {
            name: "Audio",
            extensions: ["mp3", "wav", "m4a", "flac", "ogg", "aac"],
          },
        ],
      });
      
      if (!selected || Array.isArray(selected)) return;
      
      setIsTranscribing(true);
      try {
        await invoke("transcribe_file", { filePath: selected });
      } catch (err: any) {
        setIsTranscribing(false);
        setTranscriptionError(err?.message || String(err));
        setTimeout(() => setTranscriptionError(null), 5000);
      }
    } catch (e: any) {
      setTranscriptionError(`Dialog error: ${e?.message || String(e)}`);
      setTimeout(() => setTranscriptionError(null), 5000);
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
            <OpenRecordingsButton onClick={openRecordingsFolder} />
          </div>
          <div className="px-4">
            <Button
              onClick={handleUploadAudio}
              variant="secondary"
              size="sm"
              className="w-full flex items-center justify-center gap-2"
              disabled={isTranscribing}
            >
              {isTranscribing ? (
                <>
                  <Loader2 className="w-4 h-4 animate-spin" />
                  <span>Transcribing...</span>
                </>
              ) : (
                <>
                  <Upload className="w-4 h-4" />
                  <span>Upload Audio File</span>
                </>
              )}
            </Button>
            {transcriptionError && (
              <p className="text-xs text-red-500 mt-2 text-center">{transcriptionError}</p>
            )}
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
            <OpenRecordingsButton onClick={openRecordingsFolder} />
          </div>
          <div className="px-4">
            <Button
              onClick={handleUploadAudio}
              variant="secondary"
              size="sm"
              className="w-full flex items-center justify-center gap-2"
              disabled={isTranscribing}
            >
              {isTranscribing ? (
                <>
                  <Loader2 className="w-4 h-4 animate-spin" />
                  <span>Transcribing...</span>
                </>
              ) : (
                <>
                  <Upload className="w-4 h-4" />
                  <span>Upload Audio File</span>
                </>
              )}
            </Button>
            {transcriptionError && (
              <p className="text-xs text-red-500 mt-2 text-center">{transcriptionError}</p>
            )}
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
          <OpenRecordingsButton onClick={openRecordingsFolder} />
        </div>
        <div className="px-4">
          <Button
            onClick={handleUploadAudio}
            variant="secondary"
            size="sm"
            className="w-full flex items-center justify-center gap-2"
            disabled={isTranscribing}
          >
            {isTranscribing ? (
              <>
                <Loader2 className="w-4 h-4 animate-spin" />
                <span>Transcribing...</span>
              </>
            ) : (
              <>
                <Upload className="w-4 h-4" />
                <span>Upload Audio File</span>
              </>
            )}
          </Button>
          {transcriptionError && (
            <p className="text-xs text-red-500 mt-2 text-center">{transcriptionError}</p>
          )}
        </div>
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
  getAudioUrl: (entryId: number) => Promise<AudioFileStatus | null>;
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
  const [fileExists, setFileExists] = useState(true);
  const [isUploaded, setIsUploaded] = useState(false);
  const [showCopied, setShowCopied] = useState(false);

  useEffect(() => {
    const loadAudio = async () => {
      const status = await getAudioUrl(entry.id);
      if (status) {
        setAudioUrl(status.path);
        setFileExists(status.exists);
        setIsUploaded(status.is_uploaded);
      } else {
        setAudioUrl(null);
        setFileExists(false);
        setIsUploaded(false);
      }
    };
    loadAudio();
  }, [entry.id, getAudioUrl]);

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
      alert("Failed to delete entry. Please try again.");
    }
  };

  return (
    <div className="px-4 py-2 pb-5 flex flex-col gap-3">
      <div className="flex justify-between items-center">
        <div className="flex items-center gap-2">
          {/* Icon indicator: Microphone for recordings, Document for uploads */}
          <div className="flex items-center gap-1">
            {isUploaded ? (
              <>
                <div title="Uploaded file">
                  <FileText className="w-4 h-4 text-text/50" />
                </div>
                {!fileExists && (
                  <div title="Source file missing">
                    <AlertTriangle className="w-4 h-4 text-yellow-500" />
                  </div>
                )}
              </>
            ) : (
              <div title="Recording">
                <Mic className="w-4 h-4 text-text/50" />
              </div>
            )}
          </div>
          <p className="text-sm font-medium">{entry.title}</p>
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
      {audioUrl && <AudioPlayer src={audioUrl} className="w-full" disabled={!fileExists} />}
    </div>
  );
};

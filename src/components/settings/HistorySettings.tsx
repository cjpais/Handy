import React, { useState, useEffect, useCallback } from "react";
import { SettingsGroup } from "../ui/SettingsGroup";
import { SettingContainer } from "../ui/SettingContainer";
import { ToggleSwitch } from "../ui/ToggleSwitch";
import { Button } from "../ui/Button";
import { AudioPlayer } from "../ui/AudioPlayer";
import { ClipboardCopy, Star, Check } from "lucide-react";
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useSettings } from "../../hooks/useSettings";

interface HistoryEntry {
  id: number;
  file_name: string;
  timestamp: number;
  saved: boolean;
  title: string;
  transcription_text: string;
}

export const HistorySettings: React.FC = () => {
  const [historyEntries, setHistoryEntries] = useState<HistoryEntry[]>([]);
  const [loading, setLoading] = useState(true);
  const [historySize, setHistorySize] = useState<number>(0);
  const { getSetting, updateSetting, isUpdating } = useSettings();

  const loadHistoryEntries = useCallback(async () => {
    try {
      const [entries, size] = await Promise.all([
        invoke<HistoryEntry[]>("get_history_entries"),
        invoke<number>("get_history_size")
      ]);
      setHistoryEntries(entries);
      setHistorySize(size);
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
      const unlisten = await listen("history-updated", () => {
        console.log("History updated, reloading entries...");
        loadHistoryEntries();
      });

      // Return cleanup function
      return unlisten;
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
    } catch (error) {
      console.error("Failed to copy to clipboard:", error);
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

  const handleDeleteAllHistory = async () => {
    try {
      await invoke("delete_all_history");
      setHistorySize(0);
    } catch (error) {
      console.error("Failed to delete history:", error);
    }
  };

  const formatSize = (bytes: number): string => {
    if (bytes === 0) return "0 KB";
    if (bytes < 1024) return "1 KB";
    if (bytes < 1024 * 1024) return `${Math.ceil(bytes / 1024)} KB`;
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  };

  const historyEnabled = getSetting("history_enabled") ?? true;

  return (
    <div className="max-w-3xl w-full mx-auto space-y-6">
      <SettingsGroup title="History">
        <ToggleSwitch
          checked={historyEnabled}
          onChange={(enabled) => updateSetting("history_enabled", enabled)}
          isUpdating={isUpdating("history_enabled")}
          label="Enable History"
          description="Save audio recordings and transcriptions to history"
          grouped={true}
        />
        {historyEntries.length > 0 && (
          <SettingContainer
            title={`Size on disk: ${formatSize(historySize)}`}
            description="Delete all saved recordings and transcriptions to free up space"
            grouped={true}
          >
            <Button variant="primary" size="md" onClick={handleDeleteAllHistory}>
              Delete History
            </Button>
          </SettingContainer>
        )}
        {loading ? (
          <div className="px-4 py-3 text-center text-text/60">
            Loading history...
          </div>
        ) : historyEntries.length === 0 ? (
          <div className="px-4 py-3 text-center text-text/60">
            No transcriptions yet. Start recording to build your history!
          </div>
        ) : (
          historyEntries.map((entry) => (
            <HistoryEntryComponent
              key={entry.id}
              entry={entry}
              onToggleSaved={() => toggleSaved(entry.id)}
              onCopyText={() => copyToClipboard(entry.transcription_text)}
              getAudioUrl={getAudioUrl}
            />
          ))
        )}
      </SettingsGroup>
    </div>
  );
};

interface HistoryEntryProps {
  entry: HistoryEntry;
  onToggleSaved: () => void;
  onCopyText: () => void;
  getAudioUrl: (fileName: string) => Promise<string | null>;
}

const HistoryEntryComponent: React.FC<HistoryEntryProps> = ({
  entry,
  onToggleSaved,
  onCopyText,
  getAudioUrl,
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

  return (
    <div className="px-4 py-2 pb-5 flex flex-col gap-3">
      <div className="flex justify-between items-center">
        <p className="text-sm font-medium">{entry.title}</p>
        <div className="flex items-center gap-1">
          <button
            onClick={handleCopyText}
            className="text-text/50 hover:text-logo-primary  hover:border-logo-primary transition-colors cursor-pointer"
            title="Copy transcription to clipboard"
          >
            {showCopied ? (
              <Check width={16} height={16} />
            ) : (
              <ClipboardCopy width={16} height={16} />
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
        </div>
      </div>
      <p className="italic text-text/90 text-sm pb-2">
        {entry.transcription_text}
      </p>
      {audioUrl && <AudioPlayer src={audioUrl} className="w-full" />}
    </div>
  );
};

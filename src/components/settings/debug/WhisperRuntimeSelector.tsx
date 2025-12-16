import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { listen } from "@tauri-apps/api/event";
import { SettingContainer } from "../../ui/SettingContainer";
import { Dropdown, type DropdownOption } from "../../ui/Dropdown";
import { Button } from "../../ui/Button";
import { useSettings } from "../../../hooks/useSettings";
import { commands, type WhisperRuntime } from "../../../bindings";

const WHISPER_RUNTIME_OPTIONS: DropdownOption[] = [
  { value: "whisper", label: "Whisper (Default)" },
  { value: "whisperfile", label: "Whisperfile (Experimental)" },
];

interface WhisperRuntimeSelectorProps {
  descriptionMode?: "tooltip" | "inline";
  grouped?: boolean;
}

interface DownloadProgress {
  downloaded: number;
  total: number;
  percentage: number;
}

export const WhisperRuntimeSelector: React.FC<WhisperRuntimeSelectorProps> = ({
  descriptionMode = "tooltip",
  grouped = false,
}) => {
  const { t } = useTranslation();
  const { settings, updateSetting, isUpdating } = useSettings();
  const currentRuntime = settings?.whisper_runtime ?? "whisper";

  const [isDownloaded, setIsDownloaded] = useState<boolean | null>(null);
  const [isDownloading, setIsDownloading] = useState(false);
  const [downloadProgress, setDownloadProgress] = useState<number>(0);

  // Check if binary is downloaded on mount and when runtime changes
  useEffect(() => {
    const checkDownloaded = async () => {
      try {
        const downloaded = await commands.isWhisperfileBinaryDownloaded();
        setIsDownloaded(downloaded);
      } catch (error) {
        console.error("Failed to check whisperfile status:", error);
        setIsDownloaded(false);
      }
    };
    checkDownloaded();
  }, [currentRuntime]);

  // Listen for download progress events
  useEffect(() => {
    const unlisten = listen<DownloadProgress>(
      "whisperfile-download-progress",
      (event) => {
        setDownloadProgress(Math.round(event.payload.percentage));
      }
    );

    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  const handleSelect = async (value: string) => {
    if (value === currentRuntime) return;

    try {
      await updateSetting("whisper_runtime", value as WhisperRuntime);
    } catch (error) {
      console.error("Failed to update whisper runtime:", error);
    }
  };

  const handleDownload = async () => {
    setIsDownloading(true);
    setDownloadProgress(0);
    try {
      const result = await commands.downloadWhisperfileBinary();
      if (result.status === "ok") {
        setIsDownloaded(true);
      } else {
        console.error("Failed to download whisperfile:", result.error);
      }
    } catch (error) {
      console.error("Failed to download whisperfile:", error);
    } finally {
      setIsDownloading(false);
    }
  };

  const showDownloadSection =
    currentRuntime === "whisperfile" && isDownloaded === false;

  return (
    <div className="space-y-2">
      <SettingContainer
        title={t("settings.debug.whisperRuntime.title")}
        description={t("settings.debug.whisperRuntime.description")}
        descriptionMode={descriptionMode}
        grouped={grouped}
        layout="horizontal"
      >
        <Dropdown
          options={WHISPER_RUNTIME_OPTIONS}
          selectedValue={currentRuntime}
          onSelect={handleSelect}
          disabled={!settings || isUpdating("whisper_runtime") || isDownloading}
        />
      </SettingContainer>

      {showDownloadSection && (
        <div className="ml-4 p-3 bg-orange-400/10 border border-orange-400/30 rounded-lg">
          <p className="text-sm text-text/70 mb-2">
            Whisperfile binary not found. Download required (~150 MB).
          </p>
          {isDownloading ? (
            <div className="space-y-1">
              <div className="w-full bg-mid-gray/20 rounded-full h-2">
                <div
                  className="bg-logo-primary h-2 rounded-full transition-all duration-300"
                  style={{ width: `${downloadProgress}%` }}
                />
              </div>
              <p className="text-xs text-text/60">
                Downloading... {downloadProgress}%
              </p>
            </div>
          ) : (
            <Button onClick={handleDownload} variant="primary" size="sm">
              Download Whisperfile
            </Button>
          )}
        </div>
      )}

      {currentRuntime === "whisperfile" && isDownloaded === true && (
        <div className="ml-4 p-2 bg-green-400/10 border border-green-400/30 rounded-lg">
          <p className="text-sm text-text/70">
            Whisperfile binary ready. Reload model to use.
          </p>
        </div>
      )}
    </div>
  );
};

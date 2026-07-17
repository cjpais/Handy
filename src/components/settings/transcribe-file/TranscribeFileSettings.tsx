import React, { useEffect, useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useTranslation } from "react-i18next";
import { FileAudio, Loader2, Copy, Check } from "lucide-react";
import { SettingsGroup } from "../../ui/SettingsGroup";
import { Button } from "../../ui/Button";

const AUDIO_EXTENSIONS = [
  "mp3",
  "wav",
  "m4a",
  "aac",
  "flac",
  "ogg",
  "opus",
  "wma",
  "aiff",
  "aif",
  "mp4",
  "mov",
  "webm",
  "mkv",
];

interface ProgressPayload {
  fed_samples: number;
  total_samples: number;
  fraction: number;
}

const fileNameOf = (path: string): string => path.split(/[\\/]/).pop() || path;

// Module-level cache: the component unmounts on every sidebar tab switch, and
// losing a 10-minute transcription result to a stray click is unacceptable.
const lastRun = {
  filePath: null as string | null,
  result: "",
  error: "",
};

export const TranscribeFileSettings: React.FC = () => {
  const { t } = useTranslation();
  const [filePath, setFilePath] = useState<string | null>(lastRun.filePath);
  const [isTranscribing, setIsTranscribing] = useState(false);
  const [result, setResult] = useState<string>(lastRun.result);
  const [error, setError] = useState<string>(lastRun.error);
  const [copied, setCopied] = useState(false);
  const [fraction, setFraction] = useState(0);
  const [eta, setEta] = useState<string>("");

  // Wall-clock start of the current run, used to extrapolate an ETA from how
  // far the model has progressed through the audio so far.
  const startedAt = useRef<number>(0);

  const formatClock = (seconds: number): string => {
    if (!isFinite(seconds) || seconds < 0) return "";
    const s = Math.round(seconds);
    const m = Math.floor(s / 60);
    const rem = s % 60;
    return m > 0
      ? t("transcribeFile.timeMinSec", { m, s: rem })
      : t("transcribeFile.timeSec", { s: rem });
  };

  useEffect(() => {
    const unlistenPromise = listen<ProgressPayload>(
      "transcribe-file-progress",
      (event) => {
        const { fraction } = event.payload;
        setFraction(fraction);
        if (fraction > 0.01 && startedAt.current > 0) {
          const elapsed = (Date.now() - startedAt.current) / 1000;
          const remaining = elapsed / fraction - elapsed;
          setEta(formatClock(remaining));
        }
      },
    );
    return () => {
      unlistenPromise.then((un) => un());
    };
  }, []);

  const handlePickFile = async () => {
    try {
      const selected = await open({
        multiple: false,
        directory: false,
        filters: [
          {
            name: t("transcribeFile.filterName"),
            extensions: AUDIO_EXTENSIONS,
          },
        ],
      });
      if (typeof selected === "string") {
        setFilePath(selected);
        setResult("");
        setError("");
        setFraction(0);
        setEta("");
        lastRun.filePath = selected;
        lastRun.result = "";
        lastRun.error = "";
      }
    } catch (e) {
      setError(String(e));
    }
  };

  const handleTranscribe = async () => {
    if (!filePath) return;
    setIsTranscribing(true);
    setError("");
    setResult("");
    setFraction(0);
    setEta("");
    startedAt.current = Date.now();
    try {
      const text = await invoke<string>("transcribe_audio_file", { filePath });
      setResult(text);
      setFraction(1);
      lastRun.result = text;
      lastRun.error = "";
    } catch (e) {
      const msg = typeof e === "string" ? e : String(e);
      setError(msg);
      lastRun.error = msg;
    } finally {
      setIsTranscribing(false);
      setEta("");
    }
  };

  const handleCopy = async () => {
    if (!result) return;
    try {
      await navigator.clipboard.writeText(result);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch (e) {
      console.error("Failed to copy to clipboard:", e);
    }
  };

  const pct = Math.round(fraction * 100);

  return (
    <div className="max-w-3xl w-full mx-auto space-y-6">
      <SettingsGroup
        title={t("transcribeFile.title")}
        description={t("transcribeFile.description")}
      >
        <div className="p-4 space-y-4">
          <div className="flex items-center gap-3 flex-wrap">
            <Button
              variant="secondary"
              onClick={handlePickFile}
              disabled={isTranscribing}
            >
              <span className="flex items-center gap-2">
                <FileAudio size={16} />
                {t("transcribeFile.chooseFile")}
              </span>
            </Button>
            <Button
              variant="primary"
              onClick={handleTranscribe}
              disabled={!filePath || isTranscribing}
            >
              {isTranscribing ? (
                <span className="flex items-center gap-2">
                  <Loader2 size={16} className="animate-spin" />
                  {t("transcribeFile.transcribing")}
                </span>
              ) : (
                t("transcribeFile.transcribe")
              )}
            </Button>
          </div>

          {filePath && (
            <p className="text-xs text-mid-gray truncate" title={filePath}>
              {t("transcribeFile.file", { name: fileNameOf(filePath) })}
            </p>
          )}

          {isTranscribing && (
            <div className="space-y-1">
              <div className="h-2 w-full bg-mid-gray/15 rounded-full overflow-hidden">
                <div
                  className="h-full bg-logo-primary transition-[width] duration-300"
                  style={{ width: `${pct}%` }}
                />
              </div>
              <p className="text-xs text-mid-gray">
                {pct}%
                {eta
                  ? ` · ${t("transcribeFile.remaining", { time: eta })}`
                  : ""}
              </p>
            </div>
          )}

          {error && (
            <div className="text-sm text-red-400 bg-red-500/10 border border-red-500/20 rounded-lg p-3 whitespace-pre-wrap">
              {error}
            </div>
          )}

          {result && (
            <div className="space-y-2">
              <div className="flex items-center justify-between">
                <span className="text-xs font-medium text-mid-gray uppercase tracking-wide">
                  {t("transcribeFile.result")}
                </span>
                <Button variant="ghost" size="sm" onClick={handleCopy}>
                  <span className="flex items-center gap-1">
                    {copied ? <Check size={14} /> : <Copy size={14} />}
                    {copied
                      ? t("transcribeFile.copied")
                      : t("transcribeFile.copy")}
                  </span>
                </Button>
              </div>
              <textarea
                readOnly
                value={result}
                className="w-full min-h-[160px] text-sm bg-background border border-mid-gray/20 rounded-lg p-3 resize-y select-text focus:outline-none focus:ring-1 focus:ring-logo-primary"
              />
            </div>
          )}
        </div>
      </SettingsGroup>
    </div>
  );
};

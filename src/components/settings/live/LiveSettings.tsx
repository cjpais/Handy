import React, { useEffect, useState, useCallback } from "react";
import { useTranslation } from "react-i18next";
import { listen } from "@tauri-apps/api/event";
import { TrafficLight, TrafficLightStatus } from "./TrafficLight";
import { AudioVisualizer } from "./AudioVisualizer";
import { SettingsGroup } from "../../ui/SettingsGroup";

interface FillerMatch {
  word: string;
  start_index: number;
  end_index: number;
}

interface FillerBreakdownItem {
  word: string;
  count: number;
}

interface PartialTranscription {
  text: string;
  filler_count: number;
  word_count: number;
  filler_percentage: number;
  matches: FillerMatch[];
  filler_breakdown: FillerBreakdownItem[];
}

interface FillerAnalysis {
  matches: Array<{
    word: string;
    start_index: number;
    end_index: number;
  }>;
  cleaned_text: string;
  total_words: number;
  filler_count: number;
  filler_percentage: number;
  filler_breakdown: Array<{ word: string; count: number }>;
}

export const LiveSettings: React.FC = () => {
  const { t } = useTranslation();
  const [isRecording, setIsRecording] = useState(false);
  const [isTranscribing, setIsTranscribing] = useState(false);
  const [trafficStatus, setTrafficStatus] = useState<TrafficLightStatus>("idle");
  const [partialText, setPartialText] = useState<string>("");
  const [finalText, setFinalText] = useState<string>("");
  const [fillerAnalysis, setFillerAnalysis] = useState<FillerAnalysis | null>(null);
  const [sessionFillerCount, setSessionFillerCount] = useState(0);
  const [sessionWordCount, setSessionWordCount] = useState(0);
  const [recordingDuration, setRecordingDuration] = useState(0);
  const [partialMatches, setPartialMatches] = useState<FillerMatch[]>([]);
  const [partialBreakdown, setPartialBreakdown] = useState<FillerBreakdownItem[]>([]);

  // Calculate traffic light status based on filler percentage
  const calculateTrafficStatus = useCallback((fillerPercentage: number): TrafficLightStatus => {
    if (fillerPercentage < 5) return "green";
    if (fillerPercentage < 15) return "yellow";
    return "red";
  }, []);

  // Recording duration timer
  useEffect(() => {
    let interval: NodeJS.Timeout | null = null;
    if (isRecording) {
      interval = setInterval(() => {
        setRecordingDuration((prev) => prev + 1);
      }, 1000);
    }
    return () => {
      if (interval) clearInterval(interval);
    };
  }, [isRecording]);

  // Listen for recording events
  useEffect(() => {
    const unlistenOverlay = listen<string>("show-overlay", (event) => {
      if (event.payload === "recording") {
        setIsRecording(true);
        setIsTranscribing(false);
        setPartialText("");
        setFinalText("");
        setFillerAnalysis(null);
        setSessionFillerCount(0);
        setSessionWordCount(0);
        setRecordingDuration(0);
        setPartialMatches([]);
        setPartialBreakdown([]);
        setTrafficStatus("green"); // Start optimistic
      } else if (event.payload === "transcribing") {
        setIsRecording(false);
        setIsTranscribing(true);
      }
    });

    const unlistenHide = listen("hide-overlay", () => {
      setIsRecording(false);
      setIsTranscribing(false);
    });

    // Listen for partial transcription results (chunked)
    const unlistenPartial = listen<PartialTranscription>("partial-transcription", (event) => {
      const { text, filler_count, word_count, filler_percentage, matches, filler_breakdown } = event.payload;
      setPartialText(text);
      // These are cumulative values from the full transcription so far
      setSessionFillerCount(filler_count);
      setSessionWordCount(word_count);
      setPartialMatches(matches);
      setPartialBreakdown(filler_breakdown);
      setTrafficStatus(calculateTrafficStatus(filler_percentage));
    });

    // Listen for final filler analysis
    const unlistenFiller = listen<FillerAnalysis>("filler-analysis", (event) => {
      setFillerAnalysis(event.payload);
      setTrafficStatus(calculateTrafficStatus(event.payload.filler_percentage));
    });

    // Listen for transcription result
    const unlistenTranscription = listen<string>("transcription-result", (event) => {
      setFinalText(event.payload);
    });

    return () => {
      unlistenOverlay.then((fn) => fn());
      unlistenHide.then((fn) => fn());
      unlistenPartial.then((fn) => fn());
      unlistenFiller.then((fn) => fn());
      unlistenTranscription.then((fn) => fn());
    };
  }, [calculateTrafficStatus]);

  const formatDuration = (seconds: number): string => {
    const mins = Math.floor(seconds / 60);
    const secs = seconds % 60;
    return `${mins}:${secs.toString().padStart(2, "0")}`;
  };

  const getStatusMessage = (): string => {
    if (isRecording) return t("live.status.recording");
    if (isTranscribing) return t("live.status.transcribing");
    if (finalText) return t("live.status.complete");
    return t("live.status.idle");
  };

  const getTrafficLightMessage = (): string => {
    switch (trafficStatus) {
      case "green":
        return t("live.trafficLight.green");
      case "yellow":
        return t("live.trafficLight.yellow");
      case "red":
        return t("live.trafficLight.red");
      default:
        return t("live.trafficLight.idle");
    }
  };

  // Highlight filler words in text using matches array
  const renderTextWithHighlights = (text: string, matches: FillerMatch[]) => {
    if (!matches.length || !text) return <span>{text}</span>;

    const parts: React.ReactNode[] = [];
    let lastIndex = 0;

    // Sort matches by start index
    const sortedMatches = [...matches].sort((a, b) => a.start_index - b.start_index);

    sortedMatches.forEach((match, i) => {
      // Add text before this match
      if (match.start_index > lastIndex) {
        parts.push(
          <span key={`text-${i}`}>{text.slice(lastIndex, match.start_index)}</span>
        );
      }
      // Add highlighted filler word
      parts.push(
        <span
          key={`filler-${i}`}
          className="bg-red-500/30 text-red-400 px-1 rounded"
          title={t("live.fillerWord")}
        >
          {text.slice(match.start_index, match.end_index)}
        </span>
      );
      lastIndex = match.end_index;
    });

    // Add remaining text
    if (lastIndex < text.length) {
      parts.push(<span key="text-end">{text.slice(lastIndex)}</span>);
    }

    return <>{parts}</>;
  };

  return (
    <div className="flex flex-col gap-6 max-w-3xl w-full mx-auto">
      {/* Main Live Monitor */}
      <SettingsGroup title={t("live.title")}>
        <div className="flex items-center gap-6 p-4">
          {/* Traffic Light */}
          <TrafficLight status={trafficStatus} size="lg" />

          {/* Status and Visualizer */}
          <div className="flex-1 flex flex-col gap-3">
            <div className="flex items-center justify-between">
              <div>
                <p className="text-lg font-medium">{getStatusMessage()}</p>
                <p className="text-sm text-text/60">{getTrafficLightMessage()}</p>
              </div>
              {isRecording && (
                <div className="text-2xl font-mono text-logo-primary">
                  {formatDuration(recordingDuration)}
                </div>
              )}
            </div>

            {/* Audio Visualizer */}
            <AudioVisualizer isRecording={isRecording} />
          </div>
        </div>

        {/* Live Stats */}
        {(isRecording || isTranscribing || finalText) && (
          <div className="px-4 pb-4">
            <div className="flex gap-4 text-sm">
              <div className="flex-1 bg-background-dark/30 rounded-lg p-3 text-center">
                <p className="text-2xl font-bold text-text">
                  {fillerAnalysis?.filler_count ?? sessionFillerCount}
                </p>
                <p className="text-text/60">{t("live.stats.fillerWords")}</p>
              </div>
              <div className="flex-1 bg-background-dark/30 rounded-lg p-3 text-center">
                <p className="text-2xl font-bold text-text">
                  {fillerAnalysis?.total_words ?? sessionWordCount}
                </p>
                <p className="text-text/60">{t("live.stats.totalWords")}</p>
              </div>
              <div className="flex-1 bg-background-dark/30 rounded-lg p-3 text-center">
                <p className="text-2xl font-bold text-text">
                  {(fillerAnalysis?.filler_percentage ??
                    (sessionWordCount > 0 ? (sessionFillerCount / sessionWordCount) * 100 : 0)
                  ).toFixed(1)}%
                </p>
                <p className="text-text/60">{t("live.stats.fillerPercentage")}</p>
              </div>
            </div>
          </div>
        )}
      </SettingsGroup>

      {/* Transcription Display */}
      {(partialText || finalText) && (
        <SettingsGroup title={t("live.transcription.title")}>
          <div className="p-4">
            <div className="bg-background-dark/30 rounded-lg p-4 min-h-[100px] max-h-[300px] overflow-y-auto">
              <p className="text-text leading-relaxed">
                {finalText
                  ? renderTextWithHighlights(finalText, fillerAnalysis?.matches ?? [])
                  : renderTextWithHighlights(partialText, partialMatches)}
              </p>
            </div>
          </div>
        </SettingsGroup>
      )}

      {/* Filler Word Breakdown - show during live and after final analysis */}
      {((fillerAnalysis && fillerAnalysis.filler_breakdown.length > 0) || partialBreakdown.length > 0) && (
        <SettingsGroup title={t("live.breakdown.title")}>
          <div className="p-4">
            <div className="flex flex-wrap gap-2">
              {(fillerAnalysis?.filler_breakdown ?? partialBreakdown).map(({ word, count }) => (
                <div
                  key={word}
                  className="bg-red-500/20 text-red-400 px-3 py-1 rounded-full text-sm flex items-center gap-2"
                >
                  <span>{word}</span>
                  <span className="bg-red-500/30 px-2 py-0.5 rounded-full text-xs">
                    {count}
                  </span>
                </div>
              ))}
            </div>
          </div>
        </SettingsGroup>
      )}

      {/* Instructions when idle */}
      {!isRecording && !isTranscribing && !finalText && (
        <div className="text-center text-text/60 py-8">
          <p className="text-lg mb-2">{t("live.instructions.title")}</p>
          <p className="text-sm">{t("live.instructions.description")}</p>
        </div>
      )}
    </div>
  );
};

import { useEffect, useMemo, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { AnimatePresence, motion } from "framer-motion";
import { Check, EyeOff, Mic, Square } from "lucide-react";
import { useTranslation } from "react-i18next";
import { commands } from "@/bindings";
import "./MeetingPrompt.css";

type MeetingPromptSource = "LocalDetection" | "GoogleCalendar";
type MeetingOverlayMode = "suggestion" | "recording" | "stopped" | "hidden";

interface MeetingOverlayPrompt {
  provider: string;
  title: string;
  source: MeetingPromptSource;
  start_time: string;
  join_url: string | null;
}

interface MeetingOverlaySnapshot {
  sequence: number;
  mode: MeetingOverlayMode;
  prompt: MeetingOverlayPrompt | null;
  recording_started_at: string | null;
}

const SUGGESTION_SECONDS = 8;
const SUGGESTION_DURATION_MS = SUGGESTION_SECONDS * 1000;
const STOPPED_SECONDS = 5;
const STOPPED_DURATION_MS = STOPPED_SECONDS * 1000;

function formatElapsed(startedAt: string | null) {
  if (!startedAt) return "00:00:00";
  const elapsed = Math.max(0, Date.now() - new Date(startedAt).getTime());
  const totalSeconds = Math.floor(elapsed / 1000);
  const hours = String(Math.floor(totalSeconds / 3600)).padStart(2, "0");
  const minutes = String(Math.floor((totalSeconds % 3600) / 60)).padStart(
    2,
    "0",
  );
  const seconds = String(totalSeconds % 60).padStart(2, "0");
  return `${hours}:${minutes}:${seconds}`;
}

export default function MeetingPrompt() {
  const { t } = useTranslation();
  const [snapshot, setSnapshot] = useState<MeetingOverlaySnapshot>({
    sequence: 0,
    mode: "hidden",
    prompt: null,
    recording_started_at: null,
  });
  const [elapsed, setElapsed] = useState("00:00:00");
  const [stopRequested, setStopRequested] = useState(false);

  useEffect(() => {
    let cancelled = false;

    commands.getMeetingOverlaySnapshot().then((value) => {
      if (!cancelled) {
        setSnapshot(value as MeetingOverlaySnapshot);
      }
    });

    const unlisten = listen<MeetingOverlaySnapshot>(
      "meeting-overlay-show",
      (event) => {
        if (event.payload.mode !== "stopped") {
          setStopRequested(false);
        }
        setSnapshot(event.payload);
      },
    );

    return () => {
      cancelled = true;
      unlisten.then((fn) => fn());
    };
  }, []);

  useEffect(() => {
    if (snapshot.mode !== "suggestion") return;

    const timeout = window.setTimeout(() => {
      void dismissSuggestion();
    }, SUGGESTION_DURATION_MS);

    return () => window.clearTimeout(timeout);
  }, [
    snapshot.mode,
    snapshot.sequence,
    snapshot.prompt?.start_time,
    snapshot.prompt?.join_url,
  ]);

  useEffect(() => {
    if (snapshot.mode !== "recording") return;

    setElapsed(formatElapsed(snapshot.recording_started_at));
    const interval = window.setInterval(() => {
      setElapsed(formatElapsed(snapshot.recording_started_at));
    }, 500);

    return () => window.clearInterval(interval);
  }, [snapshot.mode, snapshot.recording_started_at]);

  useEffect(() => {
    if (snapshot.mode !== "stopped") return;

    const timeout = window.setTimeout(() => {
      void commands.closeMeetingPrompt();
    }, STOPPED_DURATION_MS);

    return () => window.clearTimeout(timeout);
  }, [snapshot.mode, snapshot.sequence]);

  const detail = useMemo(() => {
    const prompt = snapshot.prompt;
    if (!prompt?.provider && !prompt?.title) {
      return t("settings.meetings.assistant.detectedMeeting");
    }

    return [prompt.provider, prompt.title].filter(Boolean).join(" · ");
  }, [snapshot.prompt, t]);

  const dismissSuggestion = async () => {
    const prompt = snapshot.prompt;
    setSnapshot((current) => ({
      ...current,
      sequence: current.sequence + 1,
      mode: "hidden",
    }));
    if (snapshot.prompt) {
      await commands.dismissMeetingPrompt(prompt as any);
    } else {
      await commands.closeMeetingPrompt();
    }
  };

  const startRecording = async () => {
    setSnapshot((current) => ({
      ...current,
      sequence: current.sequence + 1,
      mode: "recording",
      recording_started_at: new Date().toISOString(),
    }));
    const result = await commands.startMeetingRecordingFromPrompt();
    if (result.status === "error") {
      setSnapshot((current) => ({
        ...current,
        sequence: current.sequence + 1,
        mode: "hidden",
      }));
      await commands.closeMeetingPrompt();
    }
  };

  const stopRecording = async () => {
    if (stopRequested) return;
    setStopRequested(true);
    setSnapshot((current) => ({
      ...current,
      sequence: current.sequence + 1,
      mode: "stopped",
      recording_started_at: null,
    }));
    await commands.stopMeetingRecordingFromOverlay();
  };

  const hideRecording = async () => {
    await commands.hideMeetingRecordingOverlay();
  };

  const closeStoppedOverlay = async () => {
    await commands.closeMeetingPrompt();
  };

  const viewMeetingInApp = async () => {
    await commands.showPrimaryWindowCommand();
    await commands.closeMeetingPrompt();
  };

  const visible = snapshot.mode !== "hidden";

  return (
    <div className="meeting-overlay-root">
      <AnimatePresence mode="wait">
        {visible && (
          <motion.div
            key={snapshot.mode}
            className="glass-panel rounded-xl border border-outline-variant shadow-sm w-[320px] h-[80px] relative overflow-hidden group hover:bg-surface-container-low transition-colors duration-300"
            layout
            initial={{ opacity: 0, scale: 0.97, filter: "blur(6px)" }}
            animate={{ opacity: 1, scale: 1, filter: "blur(0px)" }}
            exit={{ opacity: 0, scale: 0.98, filter: "blur(4px)" }}
            transition={{ type: "spring", stiffness: 520, damping: 38 }}
          >
            {snapshot.mode === "suggestion" && (
              <div className="flex h-full items-center px-6">
                <div className="flex items-center gap-4 flex-1">
                  <div className="w-1.5 h-6 rounded-full bg-alarm"></div>
                  <div className="flex flex-col">
                    <span className="font-body-sm font-semibold text-on-surface leading-tight whitespace-nowrap">
                      {t("settings.meetings.assistant.suggestionTitle")}
                    </span>
                    <span className="font-caption text-bark-grey leading-tight mt-1">
                      {snapshot.prompt?.provider || detail}
                    </span>
                  </div>
                </div>
                <div className="w-[1px] h-8 bg-primary-container/30 flex-shrink-0 mx-4 self-center"></div>
                <button
                  className="btn-interactive flex items-center gap-2 font-status-label text-status-label text-on-surface hover:bg-stone-mist/50 transition-colors duration-300 px-2 py-1 rounded-lg hover:bg-primary-container/10"
                  onClick={startRecording}
                  type="button"
                >
                  <div className="w-2.5 h-2.5 rounded-full bg-alarm animate-pulse-dot flex-shrink-0" />
                  <span>{t("settings.meetings.assistant.record")}</span>
                </button>
              </div>
            )}

            {snapshot.mode === "recording" && (
              <div className="flex items-center justify-between px-6 h-full w-full">
                <div className="flex items-center gap-4">
                  <div className="w-3 h-3 rounded-full bg-alarm animate-pulse-dot"></div>
                  <div className="flex flex-col justify-center">
                    <span className="font-body-sm font-semibold text-on-surface leading-tight">
                      {t("settings.meetings.assistant.recordingTitle")}
                    </span>
                    <span className="font-data-mono text-[12px] text-bark-grey leading-tight mt-1 tabular-nums">
                      {elapsed}
                    </span>
                  </div>
                </div>

                <div className="flex items-center gap-4 h-[40px]">
                  <div className="w-px h-full bg-stone-mist"></div>
                  <div className="flex items-center gap-2">
                    <button
                      aria-label={t("settings.meetings.assistant.hide")}
                      className="btn-interactive text-bark-grey hover:text-on-surface hover:bg-stone-mist/50 p-2 rounded-lg flex items-center justify-center"
                      onClick={hideRecording}
                      type="button"
                      title={t("settings.meetings.assistant.hide")}
                    >
                      <EyeOff size={18} />
                    </button>
                    <button
                      className="btn-interactive flex items-center gap-1.5 px-3 py-1.5 rounded-lg font-status-label text-status-label text-error hover:bg-error/10"
                      id="stop-btn"
                      disabled={stopRequested}
                      onClick={stopRecording}
                      type="button"
                    >
                      <Square size={14} fill="currentColor" />
                      {t("settings.meetings.assistant.stopSave")}
                    </button>
                  </div>
                </div>
              </div>
            )}

            {snapshot.mode === "stopped" && (
              <div className="flex h-full items-center px-6 justify-between w-full">
                <div className="flex items-center gap-4 flex-1">
                  <div
                    className="w-8 h-8 rounded-full bg-primary/10 flex items-center justify-center text-primary z-10 relative anim-scale-in"
                    id="success-icon-container"
                  >
                    <svg
                      className="w-5 h-5 text-primary"
                      fill="none"
                      stroke="currentColor"
                      strokeLinecap="round"
                      strokeLinejoin="round"
                      strokeWidth="2.5"
                      viewBox="0 0 24 24"
                    >
                      <path
                        className="check-path anim-draw-check"
                        d="M5 13l4 4L19 7"
                        id="check-icon"
                      ></path>
                    </svg>
                  </div>
                  <div className="flex flex-col">
                    <span className="font-body-sm font-semibold text-on-surface leading-tight anim-slide-up">
                      {t("settings.meetings.assistant.capturedTitle")}
                    </span>
                    <span className="font-caption text-bark-grey leading-tight mt-1 anim-slide-up-delayed">
                      Saved to history
                    </span>
                  </div>
                </div>
                <div className="w-[1px] h-8 bg-primary-container/30 flex-shrink-0 mx-4 self-center"></div>
                <button
                  className="btn-interactive flex items-center gap-1.5 font-status-label text-status-label text-primary hover:bg-primary/10 transition-colors duration-300 px-3 py-1.5 rounded-lg"
                  onClick={viewMeetingInApp}
                  type="button"
                >
                  <span>{t("settings.meetings.assistant.viewMeeting")}</span>
                </button>
              </div>
            )}

            {snapshot.mode === "suggestion" && (
              <div className="meeting-overlay-progress" aria-hidden="true">
                <motion.div
                  key={`suggestion-progress-${snapshot.sequence}`}
                  className="meeting-overlay-progress-bar"
                  initial={{ scaleX: 1 }}
                  animate={{ scaleX: 0 }}
                  transition={{
                    duration: SUGGESTION_SECONDS,
                    ease: "linear",
                  }}
                />
              </div>
            )}

            {snapshot.mode === "stopped" && (
              <div className="meeting-overlay-progress" aria-hidden="true">
                <motion.div
                  key={`stopped-progress-${snapshot.sequence}`}
                  className="meeting-overlay-progress-bar"
                  initial={{ scaleX: 1 }}
                  animate={{ scaleX: 0 }}
                  transition={{
                    duration: STOPPED_SECONDS,
                    ease: "linear",
                  }}
                  onAnimationComplete={() => {
                    void closeStoppedOverlay();
                  }}
                />
              </div>
            )}
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
}

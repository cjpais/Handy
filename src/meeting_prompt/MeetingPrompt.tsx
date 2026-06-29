import { useEffect, useMemo, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { AnimatePresence, motion } from "framer-motion";
import { Check, EyeOff, Mic, Square } from "lucide-react";
import { useTranslation } from "react-i18next";
import { commands } from "@/bindings";

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

function formatElapsed(startedAt: string | null) {
  if (!startedAt) return "00:00";
  const elapsed = Math.max(0, Date.now() - new Date(startedAt).getTime());
  const totalSeconds = Math.floor(elapsed / 1000);
  const minutes = String(Math.floor(totalSeconds / 60)).padStart(2, "0");
  const seconds = String(totalSeconds % 60).padStart(2, "0");
  return `${minutes}:${seconds}`;
}

export default function MeetingPrompt() {
  const { t } = useTranslation();
  const [snapshot, setSnapshot] = useState<MeetingOverlaySnapshot>({
    sequence: 0,
    mode: "hidden",
    prompt: null,
    recording_started_at: null,
  });
  const [elapsed, setElapsed] = useState("00:00");
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
    }, 1200);

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

  const visible = snapshot.mode !== "hidden";

  return (
    <div className="meeting-overlay-root">
      <AnimatePresence mode="wait">
        {visible && (
          <motion.div
            key={snapshot.mode}
            className={`meeting-overlay-card meeting-state-${snapshot.mode}`}
            layout
            initial={{ opacity: 0, scale: 0.97, filter: "blur(6px)" }}
            animate={{ opacity: 1, scale: 1, filter: "blur(0px)" }}
            exit={{ opacity: 0, scale: 0.98, filter: "blur(4px)" }}
            transition={{ type: "spring", stiffness: 520, damping: 38 }}
          >
            <div className="meeting-overlay-main">
              <div className="meeting-overlay-icon" aria-hidden="true">
                {snapshot.mode === "stopped" ? (
                  <Check size={18} />
                ) : (
                  <Mic size={18} />
                )}
              </div>

              <div className="meeting-overlay-copy">
                <div className="meeting-overlay-title">
                  {snapshot.mode === "suggestion" &&
                    t("settings.meetings.assistant.suggestionTitle")}
                  {snapshot.mode === "recording" &&
                    t("settings.meetings.assistant.recordingTitle")}
                  {snapshot.mode === "stopped" &&
                    t("settings.meetings.assistant.capturedTitle")}
                </div>
                <div className="meeting-overlay-detail">
                  {snapshot.mode === "recording" ? elapsed : detail}
                </div>
              </div>

              {snapshot.mode === "suggestion" && (
                <div className="meeting-overlay-actions">
                  <button
                    className="meeting-overlay-btn quiet"
                    onClick={dismissSuggestion}
                    type="button"
                  >
                    {t("settings.meetings.assistant.dismiss")}
                  </button>
                  <button
                    className="meeting-overlay-btn record"
                    onClick={startRecording}
                    type="button"
                  >
                    {t("settings.meetings.assistant.record")}
                  </button>
                </div>
              )}

              {snapshot.mode === "recording" && (
                <div className="meeting-overlay-actions">
                  <button
                    aria-label={t("settings.meetings.assistant.hide")}
                    className="meeting-overlay-icon-btn"
                    onClick={hideRecording}
                    type="button"
                  >
                    <EyeOff size={15} />
                  </button>
                  <button
                    className="meeting-overlay-btn stop"
                    disabled={stopRequested}
                    onClick={stopRecording}
                    type="button"
                  >
                    <Square size={12} fill="currentColor" />
                    {t("settings.meetings.assistant.stopSave")}
                  </button>
                </div>
              )}
            </div>

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
              <motion.div
                className="meeting-overlay-captured"
                initial={{ scaleX: 0 }}
                animate={{ scaleX: 1 }}
                transition={{ duration: 0.68, ease: [0.16, 1, 0.3, 1] }}
                onAnimationComplete={() => {
                  void closeStoppedOverlay();
                }}
              />
            )}
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
}

import { useEffect, useMemo, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { listen } from "@tauri-apps/api/event";
import { useTranslation } from "react-i18next";
import { commands } from "@/bindings";

type MeetingPromptSource = "LocalDetection" | "GoogleCalendar";

interface MeetingPromptPayload {
  provider: string;
  title: string;
  source: MeetingPromptSource;
  start_time: string;
  join_url: string | null;
}

export default function MeetingPrompt() {
  const { t } = useTranslation();
  const [payload, setPayload] = useState<MeetingPromptPayload | null>(null);

  useEffect(() => {
    const unlisten = listen<MeetingPromptPayload>(
      "meeting-prompt-show",
      (event) => {
        setPayload(event.payload);
      },
    );
    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  const formattedTime = useMemo(() => {
    if (!payload) return "";
    const date = new Date(payload.start_time);
    return date.toLocaleTimeString([], { hour: "numeric", minute: "2-digit" });
  }, [payload]);

  const sourceLabel =
    payload?.source === "GoogleCalendar"
      ? t("settings.meetings.assistant.calendarSource")
      : t("settings.meetings.assistant.localSource");

  const dismiss = async () => {
    if (payload) {
      await commands.dismissMeetingPrompt(payload as any);
    } else {
      await commands.closeMeetingPrompt();
    }
    await getCurrentWindow().hide();
  };

  const startRecording = async () => {
    await commands.startMeetingRecordingFromPrompt();
    await getCurrentWindow().hide();
  };

  const openMeeting = async () => {
    if (payload?.join_url) {
      window.open(payload.join_url, "_blank", "noopener,noreferrer");
    }
  };

  if (!payload) {
    return null;
  }

  return (
    <div className="meeting-prompt-root">
      <div className="meeting-prompt-card">
        <div>
          <div className="meeting-prompt-meta">
            <span>{payload.provider}</span>
            <span>{formattedTime}</span>
          </div>
          <p className="meeting-prompt-title">{payload.title}</p>
          <p className="meeting-prompt-subtitle">
            {t("settings.meetings.assistant.promptBody", {
              source: sourceLabel,
            })}
          </p>
        </div>
        <div className="meeting-prompt-actions">
          {payload.join_url && (
            <button
              className="meeting-prompt-btn secondary"
              onClick={openMeeting}
            >
              {t("settings.meetings.assistant.openMeeting")}
            </button>
          )}
          <button className="meeting-prompt-btn secondary" onClick={dismiss}>
            {t("settings.meetings.assistant.dismiss")}
          </button>
          <button
            className="meeting-prompt-btn primary"
            onClick={startRecording}
          >
            {t("settings.meetings.assistant.startRecording")}
          </button>
        </div>
      </div>
    </div>
  );
}

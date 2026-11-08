import type { TFunction } from "i18next";
import { platform } from "@tauri-apps/plugin-os";
import type { NoticeCode } from "@/bindings";

interface NoticeDisplay {
  title: string;
  // Without one, the Live overlay shows the notice's detail instead.
  explanation?: (t: TFunction) => string;
}

// See "Error notices" in AGENTS.md for how to add a code.
const NOTICE_DISPLAY: Record<NoticeCode, NoticeDisplay> = {
  TranscriptionFailed: {
    title: "errors.transcriptionFailedTitle",
  },
  PasteFailed: {
    title: "errors.pasteFailedTitle",
    explanation: (t) => t("errors.pasteFailed"),
  },
  NoMicrophone: {
    title: "errors.noInputDeviceTitle",
    explanation: (t) => t("errors.noInputDevice"),
  },
  MicPermissionDenied: {
    title: "errors.micPermissionDeniedTitle",
    explanation: (t) =>
      t(`errors.micPermissionDenied.${platform()}`, {
        defaultValue: t("errors.micPermissionDenied.generic"),
      }),
  },
  RecordingFailed: {
    title: "errors.recordingFailedTitle",
  },
  CapturedSilence: {
    title: "errors.capturedSilenceTitle",
    explanation: (t) => t("errors.capturedSilence"),
  },
  ModelLoadFailed: {
    title: "errors.modelLoadFailedTitle",
    explanation: (t) => t("errors.modelLoadFailed"),
  },
  PostProcessFailed: {
    title: "errors.postProcessFailedTitle",
    explanation: (t) => t("errors.postProcessFailed"),
  },
  FileTranscriptionFailed: {
    title: "errors.fileTranscriptionFailedTitle",
  },
  FilePostProcessFailed: {
    title: "errors.filePostProcessFailedTitle",
    explanation: (t) => t("errors.filePostProcessFailed"),
  },
};

export const noticeDisplay = (code: NoticeCode) => NOTICE_DISPLAY[code];

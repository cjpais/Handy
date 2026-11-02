import type { TFunction } from "i18next";
import { platform } from "@tauri-apps/plugin-os";
import type { NoticeCode } from "@/bindings";
import type { SidebarSection } from "@/components/Sidebar";

interface NoticeDisplay {
  title: string;
  // Without one, the notice's detail is shown instead.
  explanation?: (t: TFunction) => string;
  page: SidebarSection;
}

// See "Error notices" in AGENTS.md for how to add a code.
const NOTICE_DISPLAY: Record<NoticeCode, NoticeDisplay> = {
  TranscriptionFailed: {
    title: "errors.transcriptionFailedTitle",
    page: "history",
  },
  PasteFailed: {
    title: "errors.pasteFailedTitle",
    explanation: (t) => t("errors.pasteFailed"),
    page: "general",
  },
  NoMicrophone: {
    title: "errors.noInputDeviceTitle",
    explanation: (t) => t("errors.noInputDevice"),
    page: "general",
  },
  MicPermissionDenied: {
    title: "errors.micPermissionDeniedTitle",
    explanation: (t) =>
      t(`errors.micPermissionDenied.${platform()}`, {
        defaultValue: t("errors.micPermissionDenied.generic"),
      }),
    page: "general",
  },
  RecordingFailed: {
    title: "errors.recordingFailedTitle",
    page: "general",
  },
  ModelLoadFailed: {
    title: "errors.modelLoadFailedTitle",
    page: "models",
  },
  PostProcessFailed: {
    title: "errors.postProcessFailedTitle",
    explanation: (t) => t("errors.postProcessFailed"),
    page: "postprocessing",
  },
  FileTranscriptionFailed: {
    title: "settings.history.transcribeFailed",
    page: "history",
  },
  FilePostProcessFailed: {
    title: "errors.filePostProcessFailedTitle",
    explanation: (t) => t("errors.filePostProcessFailed"),
    page: "history",
  },
};

export const noticeDisplay = (code: NoticeCode) => NOTICE_DISPLAY[code];

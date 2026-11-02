import type { TFunction } from "i18next";
import { platform } from "@tauri-apps/plugin-os";
import { toast } from "sonner";
import type { Notice, NoticeCode } from "@/bindings";
import type { SidebarSection } from "@/components/Sidebar";

interface NoticeDisplay {
  title: string;
  // The longer text. Without one, the notice's detail stands in for it.
  explanation?: (t: TFunction) => string;
  // The page that helps with this notice.
  page: SidebarSection;
}

// Every NoticeCode must have an entry, so a new code can't ship without text.
// See `notices.rs` for how to add one.
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
};

export const noticeDisplay = (code: NoticeCode) => NOTICE_DISPLAY[code];

export const showNoticeToast = (t: TFunction, notice: Notice) => {
  switch (notice.code) {
    // The model-state-changed listener in App.tsx already toasts every failed
    // load, including those from Settings, which aren't notices.
    case "ModelLoadFailed":
      return;
    case "RecordingFailed":
      toast.error(
        t("errors.recordingFailed", {
          error: notice.detail ?? "Unknown error",
        }),
      );
      return;
  }

  const { title, explanation } = noticeDisplay(notice.code);
  toast.error(t(title), {
    description: explanation?.(t) ?? notice.detail,
  });
};

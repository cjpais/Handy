import type { TFunction } from "i18next";
import { toast } from "sonner";
import type { Notice } from "@/bindings";
import { noticeDisplay } from "@/lib/notices";

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
  const show = notice.level === "warning" ? toast.warning : toast.error;
  show(t(title), {
    description: explanation?.(t) ?? notice.detail,
  });
};

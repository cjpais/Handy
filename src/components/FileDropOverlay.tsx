import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import HandyHand from "./icons/HandyHand";
import { transcribeFiles } from "./settings/history/transcribeFiles";

interface FileDropOverlayProps {
  // Called once the dropped files are added, to show them in History.
  onDropped: () => void;
}

// Files dropped anywhere on the window are transcribed into History.
export const FileDropOverlay: React.FC<FileDropOverlayProps> = ({
  onDropped,
}) => {
  const { t } = useTranslation();
  const [dragging, setDragging] = useState(false);

  useEffect(() => {
    const unlisten = getCurrentWebview().onDragDropEvent(async (event) => {
      const { payload } = event;
      if (payload.type === "enter") {
        setDragging(payload.paths.length > 0);
      } else if (payload.type === "leave") {
        setDragging(false);
      } else if (payload.type === "drop") {
        setDragging(false);
        if (payload.paths.length === 0) return;
        if ((await transcribeFiles(payload.paths)) > 0) onDropped();
      }
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, [onDropped]);

  if (!dragging) return null;

  return (
    <div className="fixed inset-0 z-50 p-3 bg-background/85 pointer-events-none">
      <div className="h-full w-full flex flex-col items-center justify-center gap-2 rounded-xl border-2 border-dashed border-logo-primary text-center">
        <HandyHand width={44} height={47} />
        <p className="text-base font-semibold">
          {t("settings.history.dropTitle")}
        </p>
        <p className="text-sm text-ink-2">
          {t("settings.history.dropDescription")}
        </p>
      </div>
    </div>
  );
};

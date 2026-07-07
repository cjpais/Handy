import { listen } from "@tauri-apps/api/event";
import React, { useEffect, useState } from "react";
import "./RecordingOverlay.css";
import { LiveWaveform } from "@/components/ui";
import i18n, { syncLanguageFromSettings } from "@/i18n";
import { getLanguageDirection } from "@/lib/utils/rtl";

type OverlayState = "recording" | "transcribing" | "processing";

const RecordingOverlay: React.FC = () => {
  const [isVisible, setIsVisible] = useState(false);
  const [state, setState] = useState<OverlayState>("recording");
  const [isMeeting, setIsMeeting] = useState(false);
  const direction = getLanguageDirection(i18n.language);

  useEffect(() => {
    let unlistenShow: (() => void) | null = null;
    let unlistenHide: (() => void) | null = null;
    let unlistenMode: (() => void) | null = null;

    const setupEventListeners = async () => {
      // Listen for show-overlay event from Rust
      unlistenShow = await listen("show-overlay", async (event) => {
        // Sync language from settings each time overlay is shown
        await syncLanguageFromSettings();
        const overlayState = event.payload as OverlayState;
        setState(overlayState);
        setIsVisible(true);
      });

      // Listen for hide-overlay event from Rust
      unlistenHide = await listen("hide-overlay", () => {
        setIsVisible(false);
        setIsMeeting(false);
      });

      // Listen for recording mode changes to know if we're in meeting mode
      unlistenMode = await listen<{
        mode: "meeting" | "transcribe" | "idle";
      }>("recording-state-changed", (event) => {
        setIsMeeting(event.payload.mode === "meeting");
      });
    };

    setupEventListeners();

    // Correctly return a synchronous cleanup function
    return () => {
      if (unlistenShow) unlistenShow();
      if (unlistenHide) unlistenHide();
      if (unlistenMode) unlistenMode();
    };
  }, []);

  const isActive = state === "recording";
  const isProcessing = state === "transcribing" || state === "processing";

  return (
    <div
      dir={direction}
      className={`recording-overlay ${isVisible ? "fade-in" : ""} ${
        isMeeting ? "meeting" : ""
      }`}
    >
      <LiveWaveform
        active={isActive}
        processing={isProcessing}
        height={40}
        barWidth={3}
        barGap={2}
        mode="static"
        fadeEdges={true}
        barColor={isMeeting ? "var(--color-terracotta)" : "var(--color-forest-green)"}
      />
    </div>
  );
};

export default RecordingOverlay;

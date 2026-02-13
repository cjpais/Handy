import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import React, { useEffect, useState } from "react";
import { LiveWaveform } from "@/components/ui/live-waveform"
import "./RecordingOverlay.css";
import i18n, { syncLanguageFromSettings } from "@/i18n";
import { getLanguageDirection } from "@/lib/utils/rtl";
import { commands } from "@/bindings";

type OverlayState = "recording" | "transcribing" | "processing";

const RecordingOverlay: React.FC = () => {
  const [isVisible, setIsVisible] = useState(false);
  const [state, setState] = useState<OverlayState>("recording");
  const direction = getLanguageDirection(i18n.language);

  const handleMouseDown = async () => {
    const currentWindow = getCurrentWindow();
    try {
      await currentWindow.startDragging();
    } catch (error) {
      console.error("Failed to start dragging:", error);
    }
  };

  useEffect(() => {
    const setupEventListeners = async () => {
      // Listen for show-overlay event from Rust
      const unlistenShow = await listen("show-overlay", async (event) => {
        // Sync language from settings each time overlay is shown
        await syncLanguageFromSettings();
        const overlayState = event.payload as OverlayState;
        setState(overlayState);
        setIsVisible(true);
      });

      // Listen for hide-overlay event from Rust
      const unlistenHide = await listen("hide-overlay", () => {
        setIsVisible(false);
      });

      // Listen for window moved event to save position
      const currentWindow = getCurrentWindow();
      const unlistenMoved = await currentWindow.onMoved(async ({ payload }) => {
        // Save the new position when user drags the overlay
        try {
          await commands.saveOverlayPosition(payload.x, payload.y);
        } catch (error) {
          console.error("Failed to save overlay position:", error);
        }
      });

      // Cleanup function
      return () => {
        unlistenShow();
        unlistenHide();
        unlistenMoved();
      };
    };

    setupEventListeners();
  }, []);



  return (
    <div
      dir={direction}
      className={`recording-overlay ${isVisible ? "fade-in" : ""}`}
      onMouseDown={handleMouseDown}
    >
      <div className="waveform-container">
        <LiveWaveform
          mode="scrolling"
          active={state === "recording"}
          processing={state === "transcribing"}
          barColor="#ffffff"
          barGap={1}
          barWidth={2}
          height={60}
          fadeEdges={true}
        />
      </div>
    </div>
  );
};

export default RecordingOverlay;

import { listen } from "@tauri-apps/api/event";
import React, { useEffect, useState } from "react";
import { LiveWaveform } from "@/components/ui/live-waveform"
import "./RecordingOverlay.css";
import i18n, { syncLanguageFromSettings } from "@/i18n";
import { getLanguageDirection } from "@/lib/utils/rtl";

type OverlayState = "recording" | "transcribing";

const RecordingOverlay: React.FC = () => {
  const [isVisible, setIsVisible] = useState(false);
  const [state, setState] = useState<OverlayState>("recording");
  const direction = getLanguageDirection(i18n.language);

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

      // Cleanup function
      return () => {
        unlistenShow();
        unlistenHide();
      };
    };

    setupEventListeners();
  }, []);



  return (
    <div
      dir={direction}
      className={`recording-overlay ${isVisible ? "fade-in" : ""}`}
    >
      <div className="waveform-container">
        <LiveWaveform
          mode="scrolling"
          active={state === "recording"}
          processing={state === "transcribing"}
          barColor="#ffffff"
          barGap={1}
          barWidth={2}
          fadeEdges={true}
        />
      </div>
    </div>
  );
};

export default RecordingOverlay;

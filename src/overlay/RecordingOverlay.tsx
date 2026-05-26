import { listen } from "@tauri-apps/api/event";
import { Check, FileText, Mic, X } from "lucide-react";
import React, { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import "./RecordingOverlay.css";
import { commands, type OverlayTheme } from "@/bindings";
import i18n, { syncLanguageFromSettings } from "@/i18n";
import { getLanguageDirection } from "@/lib/utils/rtl";

type OverlayState = "recording" | "transcribing" | "processing";

const themeIconColor: Record<OverlayTheme, string> = {
  calm: "#2f5f73",
  classic: "#faa2ca",
  dark: "#8bb9c9",
};

const RecordingOverlay: React.FC = () => {
  const { t } = useTranslation();
  const [isVisible, setIsVisible] = useState(false);
  const [state, setState] = useState<OverlayState>("recording");
  const [theme, setTheme] = useState<OverlayTheme>("calm");
  const [levels, setLevels] = useState<number[]>(Array(16).fill(0));
  const smoothedLevelsRef = useRef<number[]>(Array(16).fill(0));
  const direction = getLanguageDirection(i18n.language);

  useEffect(() => {
    const loadTheme = async () => {
      const result = await commands.getAppSettings();
      if (result.status === "ok") {
        setTheme(result.data.overlay_theme ?? "calm");
      }
    };

    const setupEventListeners = async () => {
      // Listen for show-overlay event from Rust
      const unlistenShow = await listen("show-overlay", async (event) => {
        // Sync language from settings each time overlay is shown
        await syncLanguageFromSettings();
        await loadTheme();
        const overlayState = event.payload as OverlayState;
        setState(overlayState);
        setIsVisible(true);
      });

      // Listen for hide-overlay event from Rust
      const unlistenHide = await listen("hide-overlay", () => {
        setIsVisible(false);
      });

      // Listen for mic-level updates
      const unlistenLevel = await listen<number[]>("mic-level", (event) => {
        const newLevels = event.payload as number[];

        // Apply smoothing to reduce jitter
        const smoothed = smoothedLevelsRef.current.map((prev, i) => {
          const target = newLevels[i] || 0;
          return prev * 0.7 + target * 0.3; // Smooth transition
        });

        smoothedLevelsRef.current = smoothed;
        setLevels(smoothed.slice(0, 9));
      });

      // Cleanup function
      return () => {
        unlistenShow();
        unlistenHide();
        unlistenLevel();
      };
    };

    setupEventListeners();
  }, []);

  const getIcon = () => {
    const iconColor = themeIconColor[theme];

    if (state === "recording") {
      return <Mic size={18} strokeWidth={2.2} color={iconColor} />;
    }

    return <FileText size={18} strokeWidth={2.2} color={iconColor} />;
  };

  return (
    <div
      dir={direction}
      className={`recording-overlay recording-overlay-${theme} ${
        isVisible ? "fade-in" : ""
      }`}
    >
      <div className="overlay-left">{getIcon()}</div>

      <div className="overlay-middle">
        {state === "recording" && (
          <div className="bars-container">
            {levels.map((v, i) => (
              <div
                key={i}
                className="bar"
                style={{
                  height: `${Math.min(20, 4 + Math.pow(v, 0.7) * 16)}px`, // Cap at 20px max height
                  transition: "height 60ms ease-out, opacity 120ms ease-out",
                  opacity: Math.max(0.2, v * 1.7), // Minimum opacity for visibility
                }}
              />
            ))}
          </div>
        )}
        {state === "transcribing" && (
          <div className="transcribing-text">{t("overlay.transcribing")}</div>
        )}
        {state === "processing" && (
          <div className="transcribing-text">{t("overlay.processing")}</div>
        )}
      </div>

      <div className="overlay-right">
        {state === "recording" && (
          <div className="overlay-actions">
            <button
              type="button"
              className="overlay-action finish-button"
              aria-label={t("overlay.insertWithoutSubmit")}
              title={t("overlay.insertWithoutSubmit")}
              onClick={() => {
                commands.finishWithoutSubmit();
              }}
            >
              <Check size={16} strokeWidth={2.4} />
            </button>
            <button
              type="button"
              className="overlay-action cancel-button"
              aria-label={t("overlay.cancel")}
              title={t("overlay.cancel")}
              onClick={() => {
                commands.cancelOperation();
              }}
            >
              <X size={16} strokeWidth={2.4} />
            </button>
          </div>
        )}
      </div>
    </div>
  );
};

export default RecordingOverlay;

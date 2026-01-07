import { listen } from "@tauri-apps/api/event";
import React, { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  MicrophoneIcon,
  TranscriptionIcon,
  CancelIcon,
} from "../components/icons";
import "./RecordingOverlay.css";
import { commands } from "@/bindings";
import { syncLanguageFromSettings } from "@/i18n";
import {
  AccentTheme,
  syncThemeFromSettings,
  getThemeColors,
} from "@/theme";

type OverlayState = "recording" | "transcribing";

const RecordingOverlay: React.FC = () => {
  const { t } = useTranslation();
  const [isVisible, setIsVisible] = useState(false);
  const [state, setState] = useState<OverlayState>("recording");
  const [levels, setLevels] = useState<number[]>(Array(16).fill(0));
  const [currentTheme, setCurrentTheme] = useState<AccentTheme>("pink");
  const smoothedLevelsRef = useRef<number[]>(Array(16).fill(0));

  // Get current theme colors
  const themeColors = getThemeColors(currentTheme);

  useEffect(() => {
    // Load theme on mount
    syncThemeFromSettings().then(setCurrentTheme);

    const setupEventListeners = async () => {
      // Listen for show-overlay event from Rust
      const unlistenShow = await listen("show-overlay", async (event) => {
        // Sync language and theme from settings each time overlay is shown
        await syncLanguageFromSettings();
        const theme = await syncThemeFromSettings();
        setCurrentTheme(theme);
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

      // Listen for theme change events from main app
      const unlistenTheme = await listen<string>("theme-changed", (event) => {
        const theme = event.payload as AccentTheme;
        setCurrentTheme(theme);
      });

      // Cleanup function
      return () => {
        unlistenShow();
        unlistenHide();
        unlistenLevel();
        unlistenTheme();
      };
    };

    setupEventListeners();
  }, []);

  const getIcon = () => {
    if (state === "recording") {
      return <MicrophoneIcon color={themeColors.primary} />;
    } else {
      return <TranscriptionIcon color={themeColors.primary} />;
    }
  };

  return (
    <div className={`recording-overlay ${isVisible ? "fade-in" : ""}`}>
      <div className="overlay-left">{getIcon()}</div>

      <div className="overlay-middle">
        {state === "recording" && (
          <div className="bars-container">
            {levels.map((v, i) => (
              <div
                key={i}
                className="bar"
                style={{
                  height: `${Math.min(20, 4 + Math.pow(v, 0.7) * 16)}px`,
                  transition: "height 60ms ease-out, opacity 120ms ease-out",
                  opacity: Math.max(0.2, v * 1.7),
                  background: themeColors.light,
                }}
              />
            ))}
          </div>
        )}
        {state === "transcribing" && (
          <div className="transcribing-text">{t("overlay.transcribing")}</div>
        )}
      </div>

      <div className="overlay-right">
        {state === "recording" && (
          <div
            className="cancel-button"
            onClick={() => {
              commands.cancelOperation();
            }}
            style={
              {
                "--cancel-hover-bg": `${themeColors.primary}33`,
              } as React.CSSProperties
            }
          >
            <CancelIcon color={themeColors.primary} />
          </div>
        )}
      </div>
    </div>
  );
};

export default RecordingOverlay;

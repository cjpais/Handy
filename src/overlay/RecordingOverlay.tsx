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
  OverlayTheme,
  syncThemeFromSettings,
  syncOverlayThemeFromSettings,
  getThemeColors,
} from "@/theme";

type OverlayState = "recording" | "transcribing";

const RecordingOverlay: React.FC = () => {
  const { t } = useTranslation();
  const [isVisible, setIsVisible] = useState(false);
  const [state, setState] = useState<OverlayState>("recording");
  const [levels, setLevels] = useState<number[]>(Array(16).fill(0));
  const [currentTheme, setCurrentTheme] = useState<AccentTheme>("pink");
  const [overlayTheme, setOverlayTheme] = useState<OverlayTheme>("pill");
  const smoothedLevelsRef = useRef<number[]>(Array(16).fill(0));

  // Get current theme colors
  const themeColors = getThemeColors(currentTheme);

  useEffect(() => {
    // Load themes on mount
    syncThemeFromSettings().then(setCurrentTheme);
    syncOverlayThemeFromSettings().then(setOverlayTheme);

    const setupEventListeners = async () => {
      // Listen for show-overlay event from Rust
      const unlistenShow = await listen("show-overlay", async (event) => {
        // Sync language and themes from settings each time overlay is shown
        await syncLanguageFromSettings();
        const theme = await syncThemeFromSettings();
        const oTheme = await syncOverlayThemeFromSettings();
        setCurrentTheme(theme);
        setOverlayTheme(oTheme);
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

      // Listen for accent theme change events from main app
      const unlistenTheme = await listen<string>("theme-changed", (event) => {
        const theme = event.payload as AccentTheme;
        setCurrentTheme(theme);
      });

      // Listen for overlay theme change events from main app
      const unlistenOverlayTheme = await listen<string>(
        "overlay-theme-changed",
        (event) => {
          const theme = event.payload as OverlayTheme;
          setOverlayTheme(theme);
        }
      );

      // Cleanup function
      return () => {
        unlistenShow();
        unlistenHide();
        unlistenLevel();
        unlistenTheme();
        unlistenOverlayTheme();
      };
    };

    setupEventListeners();
  }, []);

  const getIcon = (size: number = 24) => {
    if (state === "recording") {
      return (
        <MicrophoneIcon
          width={size}
          height={size}
          color={themeColors.primary}
        />
      );
    } else {
      return (
        <TranscriptionIcon
          width={size}
          height={size}
          color={themeColors.primary}
        />
      );
    }
  };

  // Common audio bars component
  const AudioBars = ({
    barCount = 9,
    barWidth = 6,
    gap = 3,
    maxHeight = 20,
  }: {
    barCount?: number;
    barWidth?: number;
    gap?: number;
    maxHeight?: number;
  }) => (
    <div
      style={{
        display: "flex",
        flexDirection: "row",
        alignItems: "flex-end",
        justifyContent: "center",
        gap: `${gap}px`,
        height: `${maxHeight + 4}px`,
      }}
    >
      {levels.slice(0, barCount).map((v, i) => (
        <div
          key={i}
          style={{
            width: `${barWidth}px`,
            height: `${Math.min(maxHeight, 4 + Math.pow(v, 0.7) * (maxHeight - 4))}px`,
            background: themeColors.light,
            borderRadius: "2px",
            transition: "height 60ms ease-out, opacity 120ms ease-out",
            opacity: Math.max(0.2, v * 1.7),
          }}
        />
      ))}
    </div>
  );

  // Pill theme (default)
  if (overlayTheme === "pill") {
    return (
      <div className={`recording-overlay pill ${isVisible ? "fade-in" : ""}`}>
        <div className="overlay-left">{getIcon()}</div>

        <div className="overlay-middle">
          {state === "recording" && <AudioBars />}
          {state === "transcribing" && (
            <div className="transcribing-text">
              {t("overlay.transcribing")}
            </div>
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
  }

  // Minimal theme
  if (overlayTheme === "minimal") {
    return (
      <div className={`recording-overlay minimal ${isVisible ? "fade-in" : ""}`}>
        {state === "recording" && (
          <AudioBars barCount={9} barWidth={5} gap={3} maxHeight={18} />
        )}
        {state === "transcribing" && (
          <div className="transcribing-text minimal">
            {t("overlay.transcribing")}
          </div>
        )}
      </div>
    );
  }

  // Glassmorphism theme
  if (overlayTheme === "glassmorphism") {
    return (
      <div
        className={`recording-overlay glassmorphism ${isVisible ? "fade-in" : ""}`}
        style={
          {
            "--glass-border": `${themeColors.primary}40`,
            "--glass-shadow": `${themeColors.primary}20`,
          } as React.CSSProperties
        }
      >
        <div className="overlay-left">{getIcon(20)}</div>

        <div className="overlay-middle">
          {state === "recording" && (
            <AudioBars barCount={7} barWidth={5} gap={3} maxHeight={16} />
          )}
          {state === "transcribing" && (
            <div className="transcribing-text">
              {t("overlay.transcribing")}
            </div>
          )}
        </div>

        <div className="overlay-right">
          {state === "recording" && (
            <div
              className="cancel-button glass"
              onClick={() => {
                commands.cancelOperation();
              }}
              style={
                {
                  "--cancel-hover-bg": `${themeColors.primary}33`,
                } as React.CSSProperties
              }
            >
              <CancelIcon width={14} height={14} color={themeColors.primary} />
            </div>
          )}
        </div>
      </div>
    );
  }

  return null;
};

export default RecordingOverlay;

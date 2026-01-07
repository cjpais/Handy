import { listen } from "@tauri-apps/api/event";
import React, { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { OverlayDisplay } from "../components/overlay";
import "./RecordingOverlay.css";
import { commands } from "@/bindings";
import { syncLanguageFromSettings } from "@/i18n";
import {
  AccentTheme,
  OverlayTheme,
  syncThemeFromSettings,
  syncOverlayThemeFromSettings,
} from "@/theme";

type OverlayState = "recording" | "transcribing";

const RecordingOverlay: React.FC = () => {
  const { t } = useTranslation();
  const [isVisible, setIsVisible] = useState(false);
  const [state, setState] = useState<OverlayState>("recording");
  const [levels, setLevels] = useState<number[]>(Array(20).fill(0));
  const [currentTheme, setCurrentTheme] = useState<AccentTheme>("pink");
  const [overlayTheme, setOverlayTheme] = useState<OverlayTheme>("pill");
  const [showIcons, setShowIcons] = useState(true);
  const [barsCentered, setBarsCentered] = useState(false);
  const [barCount, setBarCount] = useState(9);
  const [barSize, setBarSize] = useState(6);
  const [barColor, setBarColor] = useState("accent");
  const smoothedLevelsRef = useRef<number[]>(Array(20).fill(0));

  const loadSettings = async () => {
    const result = await commands.getAppSettings();
    if (result.status === "ok") {
      setShowIcons(result.data.overlay_show_icons ?? true);
      setBarsCentered(result.data.overlay_bars_centered ?? false);
      setBarCount(result.data.overlay_bar_count ?? 9);
      setBarSize(result.data.overlay_bar_size ?? 6);
      setBarColor(result.data.overlay_bar_color ?? "accent");
    }
  };

  useEffect(() => {
    // Load themes on mount
    syncThemeFromSettings().then(setCurrentTheme);
    syncOverlayThemeFromSettings().then(setOverlayTheme);
    loadSettings();

    const setupEventListeners = async () => {
      // Listen for show-overlay event from Rust
      const unlistenShow = await listen("show-overlay", async (event) => {
        // Sync language and themes from settings each time overlay is shown
        await syncLanguageFromSettings();
        const theme = await syncThemeFromSettings();
        const oTheme = await syncOverlayThemeFromSettings();
        setCurrentTheme(theme);
        setOverlayTheme(oTheme);

        // Reload all settings
        await loadSettings();

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
        setLevels(smoothed);
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

      // Listen for icon visibility change events
      const unlistenShowIcons = await listen<boolean>(
        "overlay-show-icons-changed",
        (event) => {
          setShowIcons(event.payload);
        }
      );

      // Listen for bars centered change events
      const unlistenBarsCentered = await listen<boolean>(
        "overlay-bars-centered-changed",
        (event) => {
          setBarsCentered(event.payload);
        }
      );

      // Listen for bar count change events
      const unlistenBarCount = await listen<number>(
        "overlay-bar-count-changed",
        (event) => {
          setBarCount(event.payload);
        }
      );

      // Listen for bar color change events
      const unlistenBarColor = await listen<string>(
        "overlay-bar-color-changed",
        (event) => {
          setBarColor(event.payload);
        }
      );

      // Listen for bar size change events
      const unlistenBarSize = await listen<number>(
        "overlay-bar-size-changed",
        (event) => {
          setBarSize(event.payload);
        }
      );

      // Cleanup function
      return () => {
        unlistenShow();
        unlistenHide();
        unlistenLevel();
        unlistenTheme();
        unlistenOverlayTheme();
        unlistenShowIcons();
        unlistenBarsCentered();
        unlistenBarCount();
        unlistenBarColor();
        unlistenBarSize();
      };
    };

    setupEventListeners();
  }, []);

  const handleCancel = () => {
    commands.cancelOperation();
  };

  return (
    <div className={`recording-overlay-wrapper ${isVisible ? "fade-in" : ""}`}>
      <OverlayDisplay
        overlayTheme={overlayTheme}
        accentTheme={currentTheme}
        levels={levels}
        state={state}
        showIcons={showIcons}
        scale="full"
        onCancel={handleCancel}
        animate={true}
        transcribingText={t("overlay.transcribing")}
        barsCentered={barsCentered}
        customBarCount={barCount}
        customBarSize={barSize}
        customBarColor={barColor}
      />
    </div>
  );
};

export default RecordingOverlay;

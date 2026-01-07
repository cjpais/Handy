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
  const [levels, setLevels] = useState<number[]>(Array(16).fill(0));
  const [currentTheme, setCurrentTheme] = useState<AccentTheme>("pink");
  const [overlayTheme, setOverlayTheme] = useState<OverlayTheme>("pill");
  const [showIcons, setShowIcons] = useState(true);
  const smoothedLevelsRef = useRef<number[]>(Array(16).fill(0));

  useEffect(() => {
    // Load themes on mount
    syncThemeFromSettings().then(setCurrentTheme);
    syncOverlayThemeFromSettings().then(setOverlayTheme);

    // Load icon visibility setting
    commands.getAppSettings().then((result) => {
      if (result.status === "ok") {
        setShowIcons(result.data.overlay_show_icons ?? true);
      }
    });

    const setupEventListeners = async () => {
      // Listen for show-overlay event from Rust
      const unlistenShow = await listen("show-overlay", async (event) => {
        // Sync language and themes from settings each time overlay is shown
        await syncLanguageFromSettings();
        const theme = await syncThemeFromSettings();
        const oTheme = await syncOverlayThemeFromSettings();
        setCurrentTheme(theme);
        setOverlayTheme(oTheme);

        // Reload icon visibility
        const settingsResult = await commands.getAppSettings();
        if (settingsResult.status === "ok") {
          setShowIcons(settingsResult.data.overlay_show_icons ?? true);
        }

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

      // Listen for icon visibility change events
      const unlistenShowIcons = await listen<boolean>(
        "overlay-show-icons-changed",
        (event) => {
          setShowIcons(event.payload);
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
      />
    </div>
  );
};

export default RecordingOverlay;

import React, { useEffect, useState } from "react";
import { MicrophoneIcon } from "../../icons";
import {
  AccentTheme,
  OverlayTheme,
  getThemeColors,
} from "../../../theme";

interface OverlayPreviewProps {
  accentTheme: AccentTheme;
  overlayTheme: OverlayTheme;
  animate?: boolean;
}

// Simulated audio levels for animation
const generateLevels = (): number[] => {
  return Array(9)
    .fill(0)
    .map(() => Math.random() * 0.8 + 0.2);
};

export const OverlayPreview: React.FC<OverlayPreviewProps> = ({
  accentTheme,
  overlayTheme,
  animate = true,
}) => {
  const [levels, setLevels] = useState<number[]>(generateLevels());
  const themeColors = getThemeColors(accentTheme);

  useEffect(() => {
    if (!animate) return;

    const interval = setInterval(() => {
      setLevels(generateLevels());
    }, 150);

    return () => clearInterval(interval);
  }, [animate]);

  // Common bar component
  const AudioBars = ({ barCount = 9, barWidth = 6, gap = 3, maxHeight = 20 }: {
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
            height: `${Math.min(maxHeight, 4 + v * (maxHeight - 4))}px`,
            background: themeColors.light,
            borderRadius: "2px",
            transition: animate ? "height 100ms ease-out" : "none",
            opacity: Math.max(0.3, v),
          }}
        />
      ))}
    </div>
  );

  // Pill theme (current default)
  if (overlayTheme === "pill") {
    return (
      <div
        style={{
          display: "inline-flex",
          alignItems: "center",
          gap: "4px",
          padding: "4px 6px",
          background: "#000000cc",
          borderRadius: "18px",
        }}
      >
        <MicrophoneIcon width={12} height={12} color={themeColors.primary} />
        <AudioBars barCount={5} barWidth={3} gap={2} maxHeight={12} />
      </div>
    );
  }

  // Minimal theme - just bars, no icons
  if (overlayTheme === "minimal") {
    return (
      <div
        style={{
          display: "inline-flex",
          alignItems: "center",
          justifyContent: "center",
          padding: "4px 8px",
          background: "#00000099",
          borderRadius: "8px",
        }}
      >
        <AudioBars barCount={7} barWidth={3} gap={2} maxHeight={14} />
      </div>
    );
  }

  // Glassmorphism theme
  if (overlayTheme === "glassmorphism") {
    return (
      <div
        style={{
          display: "inline-flex",
          alignItems: "center",
          gap: "6px",
          padding: "6px 8px",
          background: "rgba(255, 255, 255, 0.15)",
          backdropFilter: "blur(10px)",
          WebkitBackdropFilter: "blur(10px)",
          borderRadius: "12px",
          border: `1px solid ${themeColors.primary}40`,
          boxShadow: `0 4px 16px ${themeColors.primary}20`,
        }}
      >
        <MicrophoneIcon width={12} height={12} color={themeColors.primary} />
        <AudioBars barCount={5} barWidth={3} gap={2} maxHeight={12} />
      </div>
    );
  }

  return null;
};

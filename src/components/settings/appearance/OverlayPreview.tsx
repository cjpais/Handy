import React, { useEffect, useState } from "react";
import { OverlayDisplay } from "../../overlay";
import { AccentTheme, OverlayTheme } from "../../../theme";

interface OverlayPreviewProps {
  accentTheme: AccentTheme;
  overlayTheme: OverlayTheme;
  showIcons?: boolean;
  animate?: boolean;
  barsCentered?: boolean;
  barCount?: number;
  barSize?: number;
  barColor?: string;
}

// Simulated audio levels for animation
const generateLevels = (): number[] => {
  return Array(20) // Generate enough for any bar count
    .fill(0)
    .map(() => Math.random() * 0.8 + 0.2);
};

export const OverlayPreview: React.FC<OverlayPreviewProps> = ({
  accentTheme,
  overlayTheme,
  showIcons = true,
  animate = true,
  barsCentered = false,
  barCount,
  barSize,
  barColor,
}) => {
  const [levels, setLevels] = useState<number[]>(generateLevels());

  useEffect(() => {
    if (!animate) return;

    const interval = setInterval(() => {
      setLevels(generateLevels());
    }, 150);

    return () => clearInterval(interval);
  }, [animate]);

  return (
    <OverlayDisplay
      overlayTheme={overlayTheme}
      accentTheme={accentTheme}
      levels={levels}
      showIcons={showIcons}
      scale="preview"
      animate={animate}
      barsCentered={barsCentered}
      customBarCount={barCount}
      customBarSize={barSize}
      customBarColor={barColor}
    />
  );
};

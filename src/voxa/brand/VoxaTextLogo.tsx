import React from "react";
import { VoxaMark } from "./VoxaMark";

interface VoxaTextLogoProps {
  width?: number;
  height?: number;
  className?: string;
}

/** Mark + wordmark. Drop-in for the upstream `HandyTextLogo` props. */
export const VoxaTextLogo: React.FC<VoxaTextLogoProps> = ({
  width,
  height,
  className,
}) => (
  <svg
    width={width}
    height={height}
    className={className}
    viewBox="0 0 250 72"
    fill="none"
    xmlns="http://www.w3.org/2000/svg"
    role="img"
    aria-label="Voxa"
  >
    <g transform="translate(0 4) scale(1)">
      <VoxaMark width={64} accent />
    </g>
    {/* eslint-disable-next-line i18next/no-literal-string */}
    <text
      x="80"
      y="52"
      fill="currentColor"
      fontSize="52"
      fontWeight="700"
      letterSpacing="-1.5"
      fontFamily="-apple-system, BlinkMacSystemFont, 'SF Pro Display', 'Segoe UI', system-ui, sans-serif"
    >
      Voxa
    </text>
  </svg>
);

export default VoxaTextLogo;

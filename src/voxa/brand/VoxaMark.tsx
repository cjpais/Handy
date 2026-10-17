import React from "react";

interface VoxaMarkProps {
  width?: number | string;
  height?: number | string;
  className?: string;
  /** Paint the bars with the accent gradient instead of `currentColor`. */
  accent?: boolean;
}

const BARS = [
  { x: 4, h: 14 },
  { x: 16, h: 26 },
  { x: 28, h: 36 },
  { x: 40, h: 26 },
  { x: 52, h: 14 },
];

/** Five-bar waveform, the Voxa mark. Square viewBox so it works as an icon. */
export const VoxaMark: React.FC<VoxaMarkProps> = ({
  width = 24,
  height,
  className,
  accent = false,
}) => (
  <svg
    width={width}
    height={height ?? width}
    viewBox="0 0 64 64"
    className={className}
    xmlns="http://www.w3.org/2000/svg"
    aria-hidden="true"
  >
    {accent && (
      <defs>
        <linearGradient id="voxa-mark-grad" x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" stopColor="#8fbaff" />
          <stop offset="1" stopColor="#4c8dff" />
        </linearGradient>
      </defs>
    )}
    {BARS.map(({ x, h }) => (
      <rect
        key={x}
        x={x}
        y={32 - h / 2}
        width="8"
        height={h}
        rx="4"
        fill={accent ? "url(#voxa-mark-grad)" : "currentColor"}
      />
    ))}
  </svg>
);

export default VoxaMark;

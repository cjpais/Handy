import React from "react";

export interface AudioBarsProps {
  levels: number[];
  barCount?: number;
  barWidth?: number;
  gap?: number;
  maxHeight?: number;
  color: string;
  animate?: boolean;
}

export const AudioBars: React.FC<AudioBarsProps> = ({
  levels,
  barCount = 9,
  barWidth = 6,
  gap = 3,
  maxHeight = 20,
  color,
  animate = true,
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
          background: color,
          borderRadius: "2px",
          transition: animate ? "height 60ms ease-out, opacity 120ms ease-out" : "none",
          opacity: Math.max(0.2, v * 1.7),
        }}
      />
    ))}
  </div>
);

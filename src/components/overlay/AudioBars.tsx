import React from "react";

export interface AudioBarsProps {
  levels: number[];
  barCount?: number;
  barWidth?: number;
  gap?: number;
  maxHeight?: number;
  color: string;
  animate?: boolean;
  centered?: boolean;
}

export const AudioBars: React.FC<AudioBarsProps> = ({
  levels,
  barCount = 9,
  barWidth = 6,
  gap = 3,
  maxHeight = 20,
  color,
  animate = true,
  centered = false,
}) => {
  const minHeight = 4;

  return (
    <div
      style={{
        display: "flex",
        flexDirection: "row",
        alignItems: "center",
        justifyContent: "center",
        gap: `${gap}px`,
        height: `${maxHeight + 4}px`,
      }}
    >
      {levels.slice(0, barCount).map((v, i) => {
        const barHeight = Math.min(maxHeight, minHeight + Math.pow(v, 0.7) * (maxHeight - minHeight));

        return (
          <div
            key={i}
            style={{
              width: `${barWidth}px`,
              height: centered ? `${maxHeight}px` : `${barHeight}px`,
              background: color,
              borderRadius: "2px",
              transition: animate ? "height 60ms ease-out, opacity 120ms ease-out, transform 60ms ease-out" : "none",
              opacity: Math.max(0.2, v * 1.7),
              // For centered mode, use scaleY transform from center
              // For bottom-aligned mode, use align-self: flex-end
              ...(centered
                ? {
                    transform: `scaleY(${barHeight / maxHeight})`,
                  }
                : {
                    alignSelf: "flex-end",
                  }),
            }}
          />
        );
      })}
    </div>
  );
};

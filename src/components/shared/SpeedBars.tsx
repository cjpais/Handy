import React from "react";

/** The wait cutoffs, in seconds: four bars under 2s, three under 3.5s, ... */
export const SPEED_CUTOFFS = [2, 3.5, 7.5, 20];

/** How many bars a wait earns (0–4); none for an unmeasured model. */
export const speedLevel = (waitSeconds: number | undefined): number =>
  waitSeconds == null
    ? 0
    : SPEED_CUTOFFS.filter((cutoff) => waitSeconds < cutoff).length;

const HEIGHTS = ["h-[5px]", "h-[8px]", "h-[11px]", "h-[14px]"];

interface SpeedBarsProps {
  level: number;
  className?: string;
}

/** Four bars filled up to the model's speed level. */
export const SpeedBars: React.FC<SpeedBarsProps> = ({
  level,
  className = "",
}) => (
  <span className={`inline-flex items-end gap-[2px] h-[14px] ${className}`}>
    {HEIGHTS.map((height, i) => (
      <i
        key={i}
        className={`w-[4px] rounded-[1px] ${height} ${i < level ? "bg-ink-2" : "bg-line"}`}
      />
    ))}
  </span>
);

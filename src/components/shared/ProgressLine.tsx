import React from "react";

interface ProgressLineProps {
  /** 0–1 */
  value: number;
  /** Magenta while it fills; warning once it stopped. */
  tone?: "progress" | "stopped";
  /** Checking: full, and pulsing while it waits. */
  busy?: boolean;
  /** The line's height and placement; 6px tall by default. */
  className?: string;
}

/**
 * A download's line: rounded, magenta on the line colour, as on Try it and
 * the model cards. The words go beside it; the line itself is for the eye.
 */
export const ProgressLine: React.FC<ProgressLineProps> = ({
  value,
  tone = "progress",
  busy = false,
  className = "h-[6px]",
}) => (
  <span
    aria-hidden="true"
    className={`block overflow-hidden rounded-full bg-line ${className}`}
  >
    <i
      className={`block h-full rounded-[inherit] transition-[width] duration-[120ms] ease-linear ${
        tone === "stopped" ? "bg-warning" : "bg-background-ui"
      } ${busy ? "animate-pulse" : ""}`}
      style={{
        width: `${(Math.min(1, Math.max(0, value)) * 100).toFixed(1)}%`,
      }}
    />
  </span>
);

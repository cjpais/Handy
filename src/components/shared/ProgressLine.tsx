import React from "react";

interface ProgressLineProps {
  /** 0–1 */
  value: number;
  /** Pulse while waiting. */
  busy?: boolean;
  /** Height and placement classes. */
  className?: string;
}

/** A decorative progress line; put the accessible text beside it. */
export const ProgressLine: React.FC<ProgressLineProps> = ({
  value,
  busy = false,
  className = "h-[6px]",
}) => (
  <span
    aria-hidden="true"
    className={`block overflow-hidden rounded-full bg-line ${className}`}
  >
    <i
      className={`block h-full rounded-[inherit] bg-background-ui transition-[width] duration-[120ms] ease-linear ${
        busy ? "animate-pulse motion-reduce:animate-none" : ""
      }`}
      style={{
        width: `${(Math.min(1, Math.max(0, value)) * 100).toFixed(1)}%`,
      }}
    />
  </span>
);

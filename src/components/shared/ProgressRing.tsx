import React from "react";

interface ProgressRingProps {
  /** 0–1 */
  value: number;
  size: number;
}

/** A small progress ring that fills clockwise from the top. */
export const ProgressRing: React.FC<ProgressRingProps> = ({ value, size }) => {
  const r = size / 2 - 2;
  const c = 2 * Math.PI * r;
  const m = size / 2;
  return (
    <svg
      width={size}
      height={size}
      viewBox={`0 0 ${size} ${size}`}
      className="shrink-0"
      aria-hidden="true"
    >
      <circle
        cx={m}
        cy={m}
        r={r}
        fill="none"
        strokeWidth="2.5"
        className="stroke-line"
      />
      <circle
        cx={m}
        cy={m}
        r={r}
        fill="none"
        strokeWidth="2.5"
        strokeLinecap="round"
        strokeDasharray={c}
        strokeDashoffset={c * (1 - Math.min(1, Math.max(0, value)))}
        transform={`rotate(-90 ${m} ${m})`}
        className="stroke-background-ui transition-[stroke-dashoffset] duration-[120ms] ease-linear"
      />
    </svg>
  );
};

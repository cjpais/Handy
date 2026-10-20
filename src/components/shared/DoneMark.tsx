import React from "react";

interface DoneMarkProps {
  className?: string;
}

/** Done, granted, ready: a 16px moss circle with a paper check. */
export const DoneMark: React.FC<DoneMarkProps> = ({ className = "" }) => (
  <span
    aria-hidden="true"
    className={`inline-grid place-items-center shrink-0 size-[16px] rounded-full bg-moss text-background ${className}`}
  >
    <svg
      width="11"
      height="11"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="3"
      strokeLinecap="round"
      strokeLinejoin="round"
    >
      <path d="M5 12.5l4.5 4.5L19 7.5" />
    </svg>
  </span>
);

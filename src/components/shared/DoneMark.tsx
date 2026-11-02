import React from "react";

/** Check mark for done, granted or ready. */
export const DoneMark: React.FC = () => (
  <span
    aria-hidden="true"
    className="inline-grid place-items-center shrink-0 size-[16px] rounded-full bg-moss text-background"
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

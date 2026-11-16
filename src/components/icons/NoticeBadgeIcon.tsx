import React from "react";

/** Colored by `currentColor`. */
export const NoticeBadgeIcon: React.FC<{ className?: string }> = ({
  className,
}) => (
  <svg className={className} viewBox="0 0 16 16" aria-hidden="true">
    <circle cx="8" cy="8" r="8" fill="currentColor" />
    <path
      d="M8 4v4.6"
      stroke="var(--color-background)"
      strokeWidth="2"
      strokeLinecap="round"
    />
    <path
      d="M8 11.8v.01"
      stroke="var(--color-background)"
      strokeWidth="2.2"
      strokeLinecap="round"
    />
  </svg>
);

import React from "react";

interface RadioProps {
  checked: boolean;
  className?: string;
}

/**
 * The mark for picking one of a few: an 18px ring, filled magenta with a
 * white check when picked. Decorative: the row around it carries the radio
 * role and the click.
 */
export const Radio: React.FC<RadioProps> = ({ checked, className = "" }) => (
  <span
    aria-hidden="true"
    className={`grid place-items-center shrink-0 size-[18px] rounded-full border-[1.5px] text-white ${
      checked ? "border-background-ui bg-background-ui" : "border-line-strong"
    } ${className}`}
  >
    {checked && (
      <svg
        width="12"
        height="12"
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        strokeWidth="2.5"
        strokeLinecap="round"
        strokeLinejoin="round"
      >
        <path d="M5 12.5l4.5 4.5L19 7.5" />
      </svg>
    )}
  </span>
);

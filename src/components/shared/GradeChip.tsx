import React from "react";

export type GradeLetter = "A" | "B" | "C" | "D" | "F" | "none";

interface GradeChipProps {
  letter: GradeLetter;
  /** What the chip says: "A+", "B−", or "—" / "?" when there's no grade. */
  label: string;
  /** Narrow, for past five languages in a row of columns. */
  dense?: boolean;
}

// Each letter's full class set, so no two classes compete for a property.
const LETTER_CLASSES: Record<GradeLetter, string> = {
  A: "text-background font-bold border-transparent bg-text",
  B: "text-text font-bold border-transparent bg-line",
  C: "text-text font-bold bg-transparent border-ink-3",
  D: "text-text font-bold bg-transparent border-dashed border-ink-3",
  F: "text-text font-bold bg-transparent border-transparent",
  none: "text-ink-3 font-medium bg-transparent border-transparent",
};

/** A model's accuracy grade for one language. */
export const GradeChip: React.FC<GradeChipProps> = ({
  letter,
  label,
  dense = false,
}) => (
  <span
    className={`inline-grid place-items-center rounded-chip border font-display leading-none h-[22px] ${dense ? "min-w-[24px] px-[3px] text-tag" : "min-w-[30px] px-[5px] text-small"} ${LETTER_CLASSES[letter]}`}
  >
    {/* Trim to cap height so the letter, is centered. */}
    <span className="block [text-box:trim-both_cap_alphabetic]">{label}</span>
  </span>
);

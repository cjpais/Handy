import React from "react";

export type GradeLetter = "A" | "B" | "C" | "D" | "F" | "none";

interface GradeChipProps {
  letter: GradeLetter;
  /** What the chip says: "A+", "B−", or "—" / "?" when there's no grade. */
  label: string;
  title?: string;
  /** Narrow, for past five languages in a row of columns. */
  dense?: boolean;
}

// Each letter's whole look, so no two classes compete for a property.
const NEUTRAL = "bg-wash border-line";
const LETTER_CLASSES: Record<GradeLetter, string> = {
  A: "text-moss font-bold bg-moss-wash border-[color-mix(in_srgb,var(--color-moss)_30%,transparent)]",
  B: `text-ink-2 font-bold ${NEUTRAL}`,
  C: `text-ink-2 font-bold ${NEUTRAL}`,
  D: `text-error font-bold ${NEUTRAL}`,
  F: `text-error font-bold ${NEUTRAL}`,
  none: `text-ink-3 font-medium ${NEUTRAL}`,
};

/** A model's accuracy for one language: A is moss, D and F are error red. */
export const GradeChip: React.FC<GradeChipProps> = ({
  letter,
  label,
  title,
  dense = false,
}) => (
  <span
    title={title}
    className={`inline-grid place-items-center rounded-chip border font-display leading-none h-[22px] ${dense ? "min-w-[24px] px-[3px] text-tag" : "min-w-[30px] px-[5px] text-small"} ${LETTER_CLASSES[letter]}`}
  >
    {label}
  </span>
);

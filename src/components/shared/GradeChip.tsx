import React from "react";
import { useTranslation } from "react-i18next";
import { Star } from "lucide-react";

export type GradeLetter = "A" | "B" | "C" | "D" | "F" | "none";

interface GradeChipProps {
  letter: GradeLetter;
  /** The grade underneath: "A+", "B−", or "—" / "?" when there's no grade. */
  label: string;
  /** Narrow, for past five languages in a row of columns. */
  dense?: boolean;
  /** Bigger stars, for when the chip has room. */
  large?: boolean;
}

const STAR_COUNT = 4;

/** Stars for a plain letter; a + adds half a star (A+ stays at 4), a − takes half off. */
const BASE_STARS: Record<Exclude<GradeLetter, "none">, number> = {
  A: 4,
  B: 3,
  C: 2,
  D: 1,
  F: 0,
};

/**
 * Stars (0 to 4, in halves) for a grade: A+ and A 4, A− and B+ 3.5, B 3,
 * B− and C+ 2.5, C 2, C− and D+ 1.5, D 1, D− 0.5, F 0. Null when ungraded.
 * Only for display: sorting still uses the measured error.
 */
export const starsFor = (letter: GradeLetter, label: string): number | null => {
  if (letter === "none") return null;
  const base = BASE_STARS[letter];
  if (letter === "F") return 0;
  const sign = label.slice(1);
  if (sign === "−" || sign === "-") return base - 0.5;
  // B+ rounds up to the half star under A; A+ has nowhere higher to go.
  if (sign === "+") return Math.min(STAR_COUNT, base + 0.5);
  return base;
};

/** The word for a letter: one i18n key each. */
export const RATING_KEYS: Record<Exclude<GradeLetter, "none">, string> = {
  A: "modelTable.rating.excellent",
  B: "modelTable.rating.good",
  C: "modelTable.rating.fair",
  D: "modelTable.rating.poor",
  F: "modelTable.rating.weak",
};

const STAR_SIZE = {
  dense: "size-[6px]",
  normal: "size-[8px]",
  large: "size-[10px]",
} as const;

/** A model's accuracy for one language, as a row of up to four stars. */
export const GradeChip: React.FC<GradeChipProps> = ({
  letter,
  label,
  dense = false,
  large = false,
}) => {
  const { t } = useTranslation();
  const stars = starsFor(letter, label);
  const size = STAR_SIZE[large ? "large" : dense ? "dense" : "normal"];
  const box = `inline-grid place-items-center rounded-chip h-[22px]`;

  if (stars === null) {
    return (
      <span
        className={`${box} min-w-[24px] px-[3px] font-display text-small font-medium leading-none text-ink-3`}
      >
        <span className="block [text-box:trim-both_cap_alphabetic]">
          {label}
        </span>
      </span>
    );
  }

  return (
    <span
      role="img"
      aria-label={t(RATING_KEYS[letter as Exclude<GradeLetter, "none">])}
      className={`${box} grid-flow-col gap-[1px]`}
    >
      {Array.from({ length: STAR_COUNT }, (_, i) => {
        const fill = Math.max(0, Math.min(1, stars - i));
        return (
          <span key={i} className={`relative inline-block ${size}`}>
            <Star
              className={`absolute inset-0 ${size} text-ink-3`}
              strokeWidth={2.5}
              aria-hidden="true"
            />
            {fill > 0 && (
              <Star
                className={`absolute inset-0 ${size} fill-current text-text`}
                strokeWidth={2.5}
                style={fill < 1 ? { clipPath: "inset(0 50% 0 0)" } : undefined}
                aria-hidden="true"
              />
            )}
          </span>
        );
      })}
    </span>
  );
};

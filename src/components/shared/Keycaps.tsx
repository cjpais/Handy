import React from "react";
import { keycapParts, type OSType } from "@/lib/utils/keyboard";

interface KeycapsProps {
  /** The binding to draw, e.g. "option+space". */
  binding: string;
  osType: OSType;
  /** sm: in sentences and steps, 30px tall; demo: under Demo's laptop,
   *  36px tall with 16px glyphs and a wider Space. */
  size?: "sm" | "demo";
  /** Every key drops by its shadow with a blush face. */
  pressed?: boolean;
  /** A combination still being captured: dashed, no drop. */
  partial?: boolean;
  /** Inside a `group/keycaps` control: the keys rise on its hover or
   *  keyboard focus, to say they can be changed. */
  lift?: boolean;
  /** Placed inside the last key (Demo's hand presses it from below). */
  lastKeyChildren?: React.ReactNode;
  className?: string;
}

const SIZES = {
  sm: {
    key: "h-[30px] px-[9px] rounded-[7px] text-small",
    narrow: "min-w-[30px]",
    wide: "",
  },
  demo: {
    key: "h-[36px] px-[11px] rounded-[9px] text-[16px]",
    narrow: "min-w-[36px]",
    wide: "min-w-[112px]",
  },
};

/**
 * The shortcut as physical keys: a paper face with a plum edge and a plum
 * drop that the key falls into when it's pressed. Handy's signature element.
 */
export const Keycaps: React.FC<KeycapsProps> = ({
  binding,
  osType,
  size = "sm",
  pressed = false,
  partial = false,
  lift = false,
  lastKeyChildren,
  className = "",
}) => {
  const parts = keycapParts(binding, osType);
  const label = parts
    .map((p) => p.label)
    .join(osType === "macos" ? " " : " + ");
  const state = pressed
    ? "translate-y-[3px] shadow-none bg-logo-primary text-[var(--light-color-logo-stroke)] border-plum"
    : partial
      ? "translate-y-[3px] shadow-none border-dashed bg-background text-plum border-plum"
      : `shadow-[0_3px_0_0_var(--color-plum)] bg-background text-plum border-plum ${
          lift
            ? "group-hover/keycaps:-translate-y-[2px] group-hover/keycaps:shadow-[0_5px_0_0_var(--color-plum)] group-focus-visible/keycaps:-translate-y-[2px] group-focus-visible/keycaps:shadow-[0_5px_0_0_var(--color-plum)]"
            : ""
        }`;

  return (
    <span
      className={`inline-flex items-center gap-[4px] align-middle ${className}`}
      role="img"
      aria-label={label}
    >
      {parts.map((part, i) => (
        <span
          key={i}
          aria-hidden="true"
          className={`relative inline-flex items-center justify-center border font-display font-semibold leading-none select-none transition-[transform,box-shadow,background-color] duration-[70ms] ease-out ${SIZES[size].key} ${part.wide ? SIZES[size].wide : SIZES[size].narrow} ${state}`}
        >
          {part.label}
          {i === parts.length - 1 && lastKeyChildren}
        </span>
      ))}
    </span>
  );
};

import React from "react";
import { keycapLabel, keycapLabels, type OSType } from "@/lib/utils/keyboard";

interface KeycapsProps {
  /** The binding to draw, e.g. "option+space". */
  binding: string;
  osType: OSType;
  /** sm: in sentences and steps, 30px tall; screen: in the corner of
   *  Demo's drawn screen, 24px tall; line: inside a line of 17px text, as
   *  in Try it's "Hold [fn] and talk.", 25px tall. */
  size?: "sm" | "screen" | "line";
  /** Every key drops by its shadow and fills with its plum. */
  pressed?: boolean;
  /** A combination still being captured: dashed, no drop. */
  partial?: boolean;
  /** Inside a `group/keycaps` control: the keys rise on its hover or
   *  keyboard focus, to say they can be changed. */
  lift?: boolean;
  className?: string;
}

const SIZES = {
  sm: "h-[30px] min-w-[30px] px-[9px] rounded-[7px] text-small",
  screen: "h-[24px] min-w-[24px] px-[7px] rounded-[6px] text-[11px]",
  line: "h-[25px] min-w-[25px] px-[7px] rounded-[6px] text-[13px]",
};

/**
 * The shortcut as physical keys: a paper face with a plum edge and a plum
 * drop that the key falls into when it's pressed, filling with the plum, its
 * letters turning paper. Handy's signature element.
 */
export const Keycaps: React.FC<KeycapsProps> = ({
  binding,
  osType,
  size = "sm",
  pressed = false,
  partial = false,
  lift = false,
  className = "",
}) => {
  const labels = keycapLabels(binding, osType);
  const state = pressed
    ? "translate-y-[3px] shadow-none bg-plum text-background border-plum"
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
      aria-label={keycapLabel(binding, osType)}
    >
      {labels.map((label, i) => (
        <span
          key={i}
          aria-hidden="true"
          className={`relative inline-flex items-center justify-center border font-display font-semibold leading-none select-none transition-[transform,box-shadow,background-color] duration-[70ms] ease-out ${SIZES[size]} ${state}`}
        >
          {label}
        </span>
      ))}
    </span>
  );
};

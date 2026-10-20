import React from "react";
import { keycapParts, type OSType } from "@/lib/utils/keyboard";

interface KeycapsProps {
  /** The binding to draw, e.g. "option+space". */
  binding: string;
  osType: OSType;
  /** sm: in sentences and steps; demo: under Demo's screen, 15px glyphs
   *  and a wider Space. Both 30px tall, as the reference draws them. */
  size?: "sm" | "demo";
  /** Every key drops by its shadow with a blush face. */
  pressed?: boolean;
  /** A combination still being captured: dashed, no drop. */
  partial?: boolean;
  /** Placed inside the last key (Demo's hand presses it from below). */
  lastKeyChildren?: React.ReactNode;
  className?: string;
}

const SIZES = {
  sm: { key: "h-[30px] px-[9px] text-small", wide: "" },
  demo: { key: "h-[30px] px-[9px] text-body", wide: "min-w-[96px]" },
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
  lastKeyChildren,
  className = "",
}) => {
  const parts = keycapParts(binding, osType);
  const label = parts
    .map((p) => p.label)
    .join(osType === "macos" ? " " : " + ");
  const state = pressed
    ? "translate-y-[3px] shadow-none bg-logo-primary text-[var(--light-color-logo-stroke)] border-logo-stroke"
    : partial
      ? "translate-y-[3px] shadow-none border-dashed bg-key-face text-key-ink border-plum-edge"
      : "shadow-[0_3px_0_0_var(--color-key-drop)] bg-key-face text-key-ink border-plum-edge";

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
          className={`relative inline-flex items-center justify-center rounded-[7px] border font-display font-semibold leading-none select-none transition-[transform,box-shadow,background-color] duration-[70ms] ease-out ${SIZES[size].key} ${part.wide ? SIZES[size].wide : "min-w-[30px]"} ${state}`}
        >
          {part.label}
          {i === parts.length - 1 && lastKeyChildren}
        </span>
      ))}
    </span>
  );
};

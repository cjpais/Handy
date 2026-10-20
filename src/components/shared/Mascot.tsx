import React from "react";
import HandyMascot from "../icons/HandyMascot";

interface MascotProps {
  /** Height in px: 54 beside the wordmark, 168 on All set. */
  size: number;
  /** Wave once on arrival, or high-five (re-key it to play again). */
  animation?: "wave" | "slap" | null;
  className?: string;
}

// The design sized the glove as an image with room around it (357 × 400,
// the glove inset 12.6% from the left and 7.25% from the top, 85% tall);
// the same box keeps every size and the wave's pivot where they were.
const BOX_ASPECT = 357 / 400;

/** Handy's glove, tilted -8deg like it's about to wave. */
export const Mascot: React.FC<MascotProps> = ({
  size,
  animation = null,
  className = "",
}) => (
  <span
    style={{ width: size * BOX_ASPECT, height: size }}
    className={`relative block origin-[50%_90%] rotate-[-8deg] select-none ${animation === "wave" ? "animate-wave" : animation === "slap" ? "animate-slap" : ""} ${className}`}
  >
    <HandyMascot className="absolute start-[12.6%] top-[7.25%] h-[85.25%] w-auto" />
  </span>
);

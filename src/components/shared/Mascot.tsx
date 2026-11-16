import React from "react";
import HandyMascot from "../icons/HandyMascot";

interface MascotProps {
  /** Height in px. */
  size: number;
  /** Wave once on arrival, or high-five (re-key it to play again). */
  animation?: "wave" | "slap" | null;
}

// The design's 357×400 image box, kept so sizes and the wave's pivot match it.
const BOX_ASPECT = 357 / 400;

/** Handy's mascot glove, optionally animated. */
export const Mascot: React.FC<MascotProps> = ({ size, animation = null }) => (
  <span
    style={{ width: size * BOX_ASPECT, height: size }}
    className={`relative block origin-[50%_90%] rotate-[-8deg] select-none ${animation === "wave" ? "animate-wave" : animation === "slap" ? "animate-slap" : ""}`}
  >
    <HandyMascot className="absolute start-[12.6%] top-[7.25%] h-[85.25%] w-auto" />
  </span>
);

import React from "react";

/** A small rounded word beside a name ("Recommended", "Old format"). */
export const Tag: React.FC<React.HTMLAttributes<HTMLSpanElement>> = ({
  className = "",
  ...props
}) => (
  <span
    className={`shrink-0 whitespace-nowrap rounded-full border px-[7px] text-tag leading-[17px] font-semibold border-line-strong text-ink-2 ${className}`}
    {...props}
  />
);

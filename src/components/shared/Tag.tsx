import React from "react";

interface TagProps extends React.HTMLAttributes<HTMLSpanElement> {
  /** On a picked row: the picked edge and full text. */
  on?: boolean;
}

/** A small rounded word beside a name ("Recommended", "Old format"). */
export const Tag: React.FC<TagProps> = ({
  on = false,
  className = "",
  ...props
}) => (
  <span
    className={`shrink-0 whitespace-nowrap rounded-full border px-[7px] text-tag leading-[17px] font-semibold ${on ? "border-plum text-text" : "border-line-strong text-ink-2"} ${className}`}
    {...props}
  />
);

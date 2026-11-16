import React from "react";

/** Small spinner for waiting on the system. */
export const Spinner: React.FC<{ className?: string }> = ({
  className = "",
}) => (
  <span
    aria-hidden="true"
    className={`inline-block size-[14px] rounded-full border-2 border-line-strong border-t-background-ui animate-spin motion-reduce:animate-none ${className}`}
  />
);

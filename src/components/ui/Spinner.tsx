import React from "react";

/** A 14px ring that spins: something is waiting on the system. */
export const Spinner: React.FC<{ className?: string }> = ({
  className = "",
}) => (
  <span
    aria-hidden="true"
    className={`inline-block size-[14px] rounded-full border-2 border-line-strong border-t-background-ui animate-spin ${className}`}
  />
);

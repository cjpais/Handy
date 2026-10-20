import React from "react";

/** A quiet action in a sentence: secondary text, underlined. */
export const TextLink: React.FC<
  React.ButtonHTMLAttributes<HTMLButtonElement>
> = ({ className = "", type = "button", ...props }) => (
  <button
    type={type}
    className={`py-[2px] text-small text-ink-2 underline decoration-line-strong underline-offset-[3px] cursor-pointer hover:text-text hover:decoration-current ${className}`}
    {...props}
  />
);

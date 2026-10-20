import React from "react";

interface InputProps extends React.InputHTMLAttributes<HTMLInputElement> {
  variant?: "default" | "compact";
}

export const Input = React.forwardRef<HTMLInputElement, InputProps>(
  ({ className = "", variant = "default", disabled, ...props }, ref) => {
    const baseClasses =
      "px-2 py-1 text-sm font-semibold bg-mid-gray/10 border border-mid-gray/80 rounded-md text-start transition-all duration-150";

    // The v2 style being compared (lib/utils/style.ts): a paper field with a
    // 1.5px edge that stays quiet on hover and focus (the caret shows focus).
    const v2Classes =
      "v2:h-[34px] v2:py-0 v2:px-[12px] v2:bg-background v2:border-[1.5px] v2:border-line-strong v2:rounded-control v2:text-body v2:font-normal v2:hover:bg-background v2:hover:border-line-strong v2:focus:bg-background v2:focus:border-line-strong v2:placeholder:text-ink-3";

    const interactiveClasses = disabled
      ? "opacity-60 cursor-not-allowed bg-mid-gray/10 border-mid-gray/40"
      : "hover:bg-logo-primary/10 hover:border-logo-primary focus:outline-none focus:bg-logo-primary/20 focus:border-logo-primary";

    const variantClasses = {
      default: "px-3 py-2",
      compact: "px-2 py-1",
    } as const;

    return (
      <input
        ref={ref}
        className={`${baseClasses} ${variantClasses[variant]} ${interactiveClasses} ${v2Classes} ${className}`}
        disabled={disabled}
        {...props}
      />
    );
  },
);

Input.displayName = "Input";

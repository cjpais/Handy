import React from "react";

interface ButtonProps extends React.ButtonHTMLAttributes<HTMLButtonElement> {
  variant?:
    | "primary"
    | "primary-soft"
    | "secondary"
    | "warning"
    | "danger"
    | "danger-ghost"
    | "ghost";
  size?: "sm" | "md" | "lg";
}

export const Button: React.FC<ButtonProps> = ({
  children,
  className = "",
  variant = "primary",
  size = "md",
  ...props
}) => {
  const baseClasses =
    "font-medium rounded-lg border focus:outline-none transition-colors disabled:opacity-50 disabled:cursor-not-allowed cursor-pointer";

  // The v2 style being compared (lib/utils/style.ts): 10px corners, 1.5px
  // edges, 600 weight, a focus outline instead of a ring, and fixed heights
  // per size (md is the flow's secondary button, lg its primary).
  const v2BaseClasses =
    "v2:inline-flex v2:items-center v2:justify-center v2:gap-2 v2:whitespace-nowrap v2:rounded-control v2:font-semibold v2:transition-[filter,background-color,border-color,color] v2:focus:ring-0 v2:focus-visible:outline-2 v2:focus-visible:outline-offset-2 v2:focus-visible:outline-background-ui v2:disabled:opacity-[0.38]";

  const variantClasses = {
    primary:
      "text-white bg-background-ui border-background-ui hover:bg-background-ui/80 hover:border-background-ui/80 focus:ring-1 focus:ring-background-ui v2:hover:bg-background-ui v2:hover:border-background-ui v2:hover:brightness-[1.06] v2:disabled:brightness-100",
    "primary-soft":
      "text-text bg-logo-primary/20 border-transparent hover:bg-logo-primary/30 focus:ring-1 focus:ring-logo-primary",
    secondary:
      "bg-mid-gray/10 border-mid-gray/20 hover:bg-background-ui/30 hover:border-logo-primary focus:outline-none v2:bg-background v2:border-line-strong v2:text-text v2:hover:bg-background v2:hover:border-plum-edge",
    // Secondary's neutral resting look, but hover/focus use the semantic
    // --color-warning token (theme.css) instead of the pink accent — for
    // buttons sitting on warning surfaces like SecureInputWarning
    warning:
      "text-text bg-mid-gray/10 border-mid-gray/20 hover:bg-warning/15 hover:border-warning focus:ring-1 focus:ring-warning v2:bg-background v2:border-line-strong v2:hover:bg-background",
    danger:
      "text-white bg-red-600 border-mid-gray/20 hover:bg-red-700 hover:border-red-700 focus:ring-1 focus:ring-red-500",
    "danger-ghost":
      "text-red-400 border-transparent hover:text-red-300 hover:bg-red-500/10 focus:bg-red-500/20",
    ghost:
      "text-current border-transparent hover:bg-mid-gray/10 hover:border-logo-primary focus:bg-mid-gray/20 v2:text-ink-2 v2:hover:bg-wash v2:hover:border-transparent v2:hover:text-text v2:focus:bg-transparent",
  };

  // v2 edges: outlined buttons and the small filled one (the next thing to
  // press, beside outlined buttons) have a 1.5px edge; a screen's large
  // primary and the quiet ghost have none. One class per button, so no two
  // widths compete.
  const v2Edge =
    (variant === "primary" && size === "lg") || variant === "ghost"
      ? "v2:border-0"
      : variant === "primary" ||
          variant === "secondary" ||
          variant === "warning"
        ? "v2:border-[1.5px]"
        : "";

  const sizeClasses = {
    sm: "px-2 py-1 text-xs v2:h-[28px] v2:py-0 v2:px-[10px] v2:text-label",
    md: "px-4 py-[5px] text-sm v2:h-[34px] v2:py-0 v2:px-[14px] v2:text-small",
    lg: "px-4 py-2 text-base v2:h-[40px] v2:py-0 v2:px-[20px] v2:text-body",
  };

  return (
    <button
      className={`${baseClasses} ${v2BaseClasses} ${variantClasses[variant]} ${sizeClasses[size]} ${v2Edge} ${className}`}
      {...props}
    >
      {children}
    </button>
  );
};

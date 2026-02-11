import React from "react";

export type IconButtonVariant = "primary" | "secondary" | "danger" | "ghost";
export type IconButtonSize = "sm" | "md" | "lg";

interface IconButtonProps extends Omit<React.ButtonHTMLAttributes<HTMLButtonElement>, "children"> {
  /** Icon element to render (e.g. <ResetIcon />). No label text. */
  icon: React.ReactNode;
  /** Accessible name for the button (required when no visible text). */
  "aria-label": string;
  variant?: IconButtonVariant;
  size?: IconButtonSize;
}

const variantClasses: Record<IconButtonVariant, string> = {
  primary:
    "text-white bg-background-ui border-background-ui hover:bg-background-ui/80 hover:border-background-ui/80 focus:ring-1 focus:ring-background-ui",
  secondary:
    "text-text bg-mid-gray/10 border-mid-gray/20 hover:bg-logo-primary/30 hover:border-logo-primary focus:outline-none focus:ring-1 focus:ring-logo-primary",
  danger:
    "text-white bg-red-600 border-mid-gray/20 hover:bg-red-700 hover:border-red-700 focus:ring-1 focus:ring-red-500",
  ghost:
    "text-current border-transparent hover:bg-mid-gray/10 hover:border-logo-primary focus:bg-mid-gray/20 focus:ring-1 focus:ring-logo-primary",
};

const sizeClasses: Record<IconButtonSize, string> = {
  sm: "p-1 rounded [&_svg]:w-3 [&_svg]:h-3",
  md: "p-2 rounded [&_svg]:w-4 [&_svg]:h-4",
  lg: "p-2 rounded [&_svg]:w-5 [&_svg]:h-5",
};

export const IconButton: React.FC<IconButtonProps> = ({
  icon,
  "aria-label": ariaLabel,
  variant = "secondary",
  size = "md",
  className = "",
  disabled = false,
  ...props
}) => (
  <button
    type="button"
    aria-label={ariaLabel}
    disabled={disabled}
    className={[
      "inline-flex items-center justify-center border border-solid font-medium transition-colors duration-150",
      "disabled:opacity-50 disabled:cursor-not-allowed cursor-pointer",
      "focus:outline-none",
      variantClasses[variant],
      sizeClasses[size],
      className,
    ].join(" ")}
    {...props}
  >
    {icon}
  </button>
);

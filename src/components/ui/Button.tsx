import React from "react";

interface ButtonProps extends React.ButtonHTMLAttributes<HTMLButtonElement> {
  variant?:
    | "primary"
    | "primary-soft"
    | "secondary"
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

  const variantClasses = {
    primary:
      "text-primary-foreground bg-primary border-primary hover:bg-primary/80 hover:border-primary/80 focus:ring-1 focus:ring-ring",
    "primary-soft":
      "text-foreground bg-accent/20 border-transparent hover:bg-accent/30 focus:ring-1 focus:ring-ring",
    secondary:
      "bg-muted/60 border-border/60 hover:bg-primary/30 hover:border-ring focus:outline-none",
    danger:
      "text-white bg-destructive border-destructive hover:bg-destructive/80 hover:border-destructive/80 focus:ring-1 focus:ring-destructive",
    "danger-ghost":
      "text-red-400 border-transparent hover:text-red-300 hover:bg-red-500/10 focus:bg-red-500/20",
    ghost:
      "text-current border-transparent hover:bg-muted/60 hover:border-ring focus:bg-muted",
  };

  const sizeClasses = {
    sm: "px-2 py-1 text-xs",
    md: "px-4 py-[5px] text-sm",
    lg: "px-4 py-2 text-base",
  };

  return (
    <button
      className={`${baseClasses} ${variantClasses[variant]} ${sizeClasses[size]} ${className}`}
      {...props}
    >
      {children}
    </button>
  );
};

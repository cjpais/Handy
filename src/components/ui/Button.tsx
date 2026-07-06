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
    "font-semibold rounded-buttons border transition-all duration-150 focus:outline-none disabled:opacity-40 disabled:cursor-not-allowed cursor-pointer inline-flex items-center justify-center gap-2 font-mono uppercase tracking-[0.04em] active:scale-[0.97]";

  const variantClasses = {
    primary:
      "text-[#fffbf7] bg-[#1d7a46] border-transparent hover:bg-[#155d35] focus:ring-[3px] focus:ring-[#1d7a46]/15",
    "primary-soft":
      "text-charcoal bg-forest-green/10 border-transparent hover:bg-forest-green/20 focus:ring-[3px] focus:ring-forest-green/15",
    secondary:
      "bg-transparent border border-stone-mist text-charcoal hover:border-charcoal hover:bg-orange-off-white/30 focus:ring-[3px] focus:ring-stone-mist/50",
    danger:
      "text-[#fffbf7] bg-alarm-red border-transparent hover:bg-red-700 focus:ring-[3px] focus:ring-alarm-red/15",
    "danger-ghost":
      "text-alarm-red border-transparent hover:bg-alarm-red/10 focus:bg-alarm-red/20",
    ghost:
      "text-charcoal border-transparent hover:bg-stone-mist/30 focus:bg-stone-mist/50",
  };

  const sizeClasses = {
    sm: "px-3 py-1 text-[11px] h-7",
    md: "px-4 py-1.5 text-[11px] h-8.5",
    lg: "px-5 py-2 text-[12px] h-10",
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

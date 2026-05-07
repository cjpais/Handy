import React from "react";

interface InputProps extends React.InputHTMLAttributes<HTMLInputElement> {
  variant?: "default" | "compact";
}

export const Input: React.FC<InputProps> = ({
  className = "",
  variant = "default",
  disabled,
  ...props
}) => {
  const baseClasses =
    "rounded-xl border border-slate-300 bg-white text-start text-sm font-medium text-slate-900 transition-all duration-150 placeholder:text-slate-400";

  const interactiveClasses = disabled
    ? "cursor-not-allowed border-slate-200 bg-slate-100 text-slate-400 opacity-60"
    : "hover:border-slate-400 focus:border-red-400 focus:outline-none focus:ring-4 focus:ring-red-100";

  const variantClasses = {
    default: "px-3 py-2",
    compact: "px-2 py-1",
  } as const;

  return (
    <input
      className={`${baseClasses} ${variantClasses[variant]} ${interactiveClasses} ${className}`}
      disabled={disabled}
      {...props}
    />
  );
};

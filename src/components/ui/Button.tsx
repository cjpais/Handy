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
    "cursor-pointer rounded-xl border font-medium transition-[background-color,border-color,color,box-shadow] disabled:cursor-not-allowed disabled:opacity-50 focus:outline-none";

  const variantClasses = {
    primary:
      "border-red-600 bg-red-600 text-white hover:border-red-700 hover:bg-red-700 focus:ring-4 focus:ring-red-100",
    "primary-soft":
      "border-red-200 bg-red-50 text-red-700 hover:border-red-300 hover:bg-red-100 focus:ring-4 focus:ring-red-100",
    secondary:
      "border-slate-300 bg-white text-slate-700 hover:border-slate-400 hover:bg-slate-50 focus:ring-4 focus:ring-slate-100",
    danger:
      "border-red-600 bg-red-600 text-white hover:border-red-700 hover:bg-red-700 focus:ring-4 focus:ring-red-100",
    "danger-ghost":
      "border-transparent text-red-600 hover:bg-red-50 hover:text-red-700 focus:bg-red-50 focus:ring-4 focus:ring-red-100",
    ghost:
      "border-transparent text-slate-600 hover:border-slate-200 hover:bg-slate-50 hover:text-slate-900 focus:bg-slate-50 focus:ring-4 focus:ring-slate-100",
  };

  const sizeClasses = {
    sm: "px-3 py-1.5 text-xs",
    md: "px-4 py-2 text-sm",
    lg: "px-5 py-2.5 text-base",
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

import React from "react";

interface ButtonProps extends React.ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: "primary" | "secondary" | "danger" | "ghost";
  size?: "sm" | "md" | "lg";
}

export const Button: React.FC<ButtonProps> = ({
  children,
  className = "",
  variant = "primary",
  size = "md",
  ...props
}) => {
  const baseClasses = "font-medium rounded focus:outline-none transition-colors disabled:opacity-50 disabled:cursor-not-allowed";
  
  const variantClasses = {
    primary: "text-white bg-background-ui hover:bg-background-ui/90 focus:ring-1 focus:ring-background-ui",
    secondary: "bg-mid-gray/10 hover:bg-background-ui/30 focus:outline-none",
    danger: "text-white bg-red-600 hover:bg-red-700 focus:ring-1 focus:ring-red-500",
    ghost: "text-current hover:bg-mid-gray/10 focus:bg-mid-gray/20"
  };

  const sizeClasses = {
    sm: "px-2 py-1 text-xs",
    md: "px-3 py-1 text-sm", 
    lg: "px-4 py-2 text-base"
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
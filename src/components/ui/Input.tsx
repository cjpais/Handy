import React from "react";

interface InputProps extends React.InputHTMLAttributes<HTMLInputElement> {
  variant?: "default" | "compact";
}

export const Input: React.FC<InputProps> = ({
  className = "",
  variant = "default",
  ...props
}) => {
  const baseClasses = "px-2 py-1 text-sm font-semibold bg-mid-gray/10 border border-mid-gray/80 rounded text-left flex items-center justify-between transition-all duration-150 hover:bg-logo-primary/10 cursor-pointer hover:border-logo-primary focus:outline-none focus:bg-logo-primary/20 focus:border-logo-primary";
  
  const variantClasses = {
    default: "px-3 py-2",
    compact: "px-2 py-1"
  };

  return (
    <input
      className={`${baseClasses} ${variantClasses[variant]} ${className}`}
      {...props}
    />
  );
};
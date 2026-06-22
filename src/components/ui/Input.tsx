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
    "px-3 py-2 text-sm bg-orange-off-white border border-stone-mist rounded-[12px] text-start transition-all duration-150 focus:outline-none";

  const interactiveClasses = disabled
    ? "opacity-40 cursor-not-allowed bg-orange-off-white border-stone-mist/50"
    : "hover:border-bark-grey focus:border-forest-green focus:ring-2 focus:ring-forest-green/25 focus:bg-orange-off-white/80";

  const variantClasses = {
    default: "px-3 py-2",
    compact: "px-2 py-1.5",
  } as const;

  return (
    <input
      className={`${baseClasses} ${variantClasses[variant]} ${interactiveClasses} ${className}`}
      disabled={disabled}
      {...props}
    />
  );
};

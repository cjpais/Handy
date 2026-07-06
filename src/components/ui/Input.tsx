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
    "bg-orange-off-white border border-stone-mist rounded-inputs text-charcoal text-start transition-all duration-150 focus:outline-none placeholder:text-pebble";

  const interactiveClasses = disabled
    ? "opacity-40 cursor-not-allowed bg-orange-off-white border-stone-mist/50"
    : "hover:border-bark-grey focus:border-forest-green focus:ring-[3px] focus:ring-forest-green/15 focus:bg-orange-off-white/80";

  const variantClasses = {
    default: "px-4 py-3 text-base",
    compact: "px-3 py-2 text-sm",
  } as const;

  return (
    <input
      className={`${baseClasses} ${variantClasses[variant]} ${interactiveClasses} ${className}`}
      disabled={disabled}
      {...props}
    />
  );
};

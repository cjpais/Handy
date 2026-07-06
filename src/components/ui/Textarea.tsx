import React from "react";

interface TextareaProps
  extends React.TextareaHTMLAttributes<HTMLTextAreaElement> {
  variant?: "default" | "compact";
}

export const Textarea: React.FC<TextareaProps> = ({
  className = "",
  variant = "default",
  ...props
}) => {
  const baseClasses =
    "bg-orange-off-white border border-stone-mist rounded-inputs text-charcoal text-start transition-all duration-150 focus:outline-none placeholder:text-pebble resize-y";

  const interactiveClasses = props.disabled
    ? "opacity-40 cursor-not-allowed bg-orange-off-white border-stone-mist/50"
    : "hover:border-bark-grey focus:border-forest-green focus:ring-[3px] focus:ring-forest-green/15 focus:bg-orange-off-white/80";

  const variantClasses = {
    default: "px-4 py-3 text-base min-h-[120px]",
    compact: "px-3 py-2 text-sm min-h-[90px]",
  };

  return (
    <textarea
      className={`${baseClasses} ${variantClasses[variant]} ${interactiveClasses} ${className}`}
      {...props}
    />
  );
};

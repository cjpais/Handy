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
    "px-2 py-1 text-sm font-semibold bg-mid-gray/10 border border-mid-gray/80 rounded-md text-start transition-[background-color,border-color] duration-150 hover:bg-logo-primary/10 hover:border-logo-primary focus:outline-none focus:bg-logo-primary/10 focus:border-logo-primary resize-y v2:px-[12px] v2:py-[8px] v2:bg-background v2:border-[1.5px] v2:border-line-strong v2:rounded-control v2:text-body v2:font-normal v2:hover:bg-background v2:hover:border-line-strong v2:focus:bg-background v2:focus:border-line-strong v2:placeholder:text-ink-3";

  const variantClasses = {
    default: "px-3 py-2 min-h-[100px]",
    compact: "px-2 py-1 min-h-[80px]",
  };

  return (
    <textarea
      className={`${baseClasses} ${variantClasses[variant]} ${className}`}
      {...props}
    />
  );
};

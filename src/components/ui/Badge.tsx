import React from "react";

interface BadgeProps {
  children: React.ReactNode;
  variant?: "primary" | "success" | "secondary";
  className?: string;
}

const Badge: React.FC<BadgeProps> = ({
  children,
  variant = "primary",
  className = "",
}) => {
  const variantClasses = {
    primary:
      "bg-forest-green/10 text-forest-green border border-forest-green/20",
    success:
      "bg-lichen-green/10 text-lichen-green border border-lichen-green/20",
    secondary: "bg-stone-mist/30 text-charcoal border border-stone-mist/50",
  };

  return (
    <span
      className={`inline-flex items-center px-2 py-0.5 rounded-tags text-[10px] font-semibold font-mono uppercase tracking-[0.04em] ${variantClasses[variant]} ${className}`}
    >
      {children}
    </span>
  );
};

export default Badge;

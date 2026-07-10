import React from "react";
import ResetIcon from "../icons/ResetIcon";

interface ResetButtonProps {
  onClick: () => void;
  disabled?: boolean;
  className?: string;
  ariaLabel?: string;
  children?: React.ReactNode;
}

export const ResetButton: React.FC<ResetButtonProps> = React.memo(
  ({ onClick, disabled = false, className = "", ariaLabel, children }) => (
    <button
      type="button"
      aria-label={ariaLabel}
      className={`p-1 rounded-md border border-transparent transition-all duration-150 ${
        disabled
          ? "opacity-50 cursor-not-allowed text-foreground/40"
          : "hover:bg-accent/30 active:bg-accent/50 active:translate-y-[1px] hover:cursor-pointer hover:border-ring text-foreground/80"
      } ${className}`}
      onClick={onClick}
      disabled={disabled}
    >
      {children ?? <ResetIcon />}
    </button>
  ),
);

import React from "react";
import ResetIcon from "../icons/ResetIcon";

interface ResetButtonProps {
  onClick: () => void;
  disabled?: boolean;
  className?: string;
}

export const ResetButton: React.FC<ResetButtonProps> = React.memo(
  ({ onClick, disabled = false, className = "" }) => (
    <button
      className={`p-1 rounded fill-text border border-transparent transition-all duration-150 ${
        disabled
          ? "opacity-50 cursor-not-allowed text-text/40"
          : "hover:bg-logo-primary/30 active:bg-logo-primary/50 active:translate-y-[1px] hover:cursor-pointer hover:border-logo-primary text-text/80"
      } ${className}`}
      onClick={onClick}
      disabled={disabled}
    >
      <ResetIcon />
    </button>
  ),
);

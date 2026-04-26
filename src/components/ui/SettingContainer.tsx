import React, { useEffect, useRef, useState } from "react";
import { Tooltip } from "./Tooltip";

interface SettingContainerProps {
  title: string;
  description: string;
  children: React.ReactNode;
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
  layout?: "horizontal" | "stacked";
  disabled?: boolean;
  tooltipPosition?: "top" | "bottom";
}

const InfoIcon = () => (
  <svg
    className="w-3.5 h-3.5 text-mid-gray cursor-help hover:text-logo-primary transition-colors duration-150 select-none shrink-0"
    fill="none"
    stroke="currentColor"
    viewBox="0 0 24 24"
    aria-label="More information"
    role="img"
  >
    <path
      strokeLinecap="round"
      strokeLinejoin="round"
      strokeWidth={2}
      d="M13 16h-1v-4h-1m1-4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z"
    />
  </svg>
);

export const SettingContainer: React.FC<SettingContainerProps> = ({
  title,
  description,
  children,
  descriptionMode = "tooltip",
  grouped = false,
  layout = "horizontal",
  disabled = false,
  tooltipPosition = "top",
}) => {
  const [showTooltip, setShowTooltip] = useState(false);
  const tooltipRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const handleClickOutside = (event: MouseEvent) => {
      if (
        tooltipRef.current &&
        !tooltipRef.current.contains(event.target as Node)
      ) {
        setShowTooltip(false);
      }
    };
    if (showTooltip) {
      document.addEventListener("mousedown", handleClickOutside);
      return () => document.removeEventListener("mousedown", handleClickOutside);
    }
  }, [showTooltip]);

  const baseClasses = grouped
    ? "px-4 py-3 transition-colors duration-150 hover:bg-white/[0.03]"
    : "px-4 py-3 rounded-xl border border-white/[0.06] bg-white/[0.04]";

  const labelClasses = `text-sm font-medium ${disabled ? "opacity-40" : "text-foreground"}`;
  const descClasses = `text-xs mt-0.5 ${disabled ? "opacity-40" : "text-mid-gray"}`;

  const TooltipWrapper = ({ children: c }: { children: React.ReactNode }) => (
    <div
      ref={tooltipRef}
      className="relative inline-flex"
      onMouseEnter={() => setShowTooltip(true)}
      onMouseLeave={() => setShowTooltip(false)}
      onClick={() => setShowTooltip((v) => !v)}
      onKeyDown={(e) => {
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          setShowTooltip((v) => !v);
        }
      }}
      role="button"
      tabIndex={0}
    >
      {c}
      {showTooltip && (
        <Tooltip targetRef={tooltipRef} position={tooltipPosition}>
          <p className="text-sm text-center leading-relaxed">{description}</p>
        </Tooltip>
      )}
    </div>
  );

  if (layout === "stacked") {
    return (
      <div className={baseClasses}>
        <div className="flex items-center gap-1.5 mb-2.5">
          <h3 className={labelClasses}>{title}</h3>
          {descriptionMode === "tooltip" ? (
            <TooltipWrapper>
              <InfoIcon />
            </TooltipWrapper>
          ) : (
            <p className={descClasses}>{description}</p>
          )}
        </div>
        <div className="w-full">{children}</div>
      </div>
    );
  }

  // Horizontal layout
  return (
    <div className={`flex items-center justify-between gap-4 ${baseClasses}`}>
      <div className="min-w-0">
        <div className="flex items-center gap-1.5">
          <h3 className={labelClasses}>{title}</h3>
          {descriptionMode === "tooltip" && (
            <TooltipWrapper>
              <InfoIcon />
            </TooltipWrapper>
          )}
        </div>
        {descriptionMode === "inline" && (
          <p className={descClasses}>{description}</p>
        )}
      </div>
      <div className="relative shrink-0">{children}</div>
    </div>
  );
};

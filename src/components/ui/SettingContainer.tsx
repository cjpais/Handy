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

  // Handle click outside to close tooltip
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
      return () =>
        document.removeEventListener("mousedown", handleClickOutside);
    }
  }, [showTooltip]);

  const toggleTooltip = () => {
    setShowTooltip(!showTooltip);
  };

  const containerClasses = grouped
    ? "px-4 p-2"
    : "px-4 p-2 rounded-lg border border-mid-gray/20";

  if (layout === "stacked") {
    if (descriptionMode === "tooltip") {
      return (
        <div className={containerClasses}>
          <div className="flex items-center gap-2 mb-2">
            <h3
              className={`text-sm font-medium ${disabled ? "opacity-50" : ""}`}
            >
              {title}
            </h3>
            <div
              ref={tooltipRef}
              className="relative"
              onMouseEnter={() => setShowTooltip(true)}
              onMouseLeave={() => setShowTooltip(false)}
              onClick={toggleTooltip}
            >
              <svg
                className="w-4 h-4 text-mid-gray cursor-help hover:text-logo-primary transition-colors duration-200 select-none"
                fill="none"
                stroke="currentColor"
                viewBox="0 0 24 24"
                aria-label="More information"
                role="button"
                tabIndex={0}
                onKeyDown={(e) => {
                  if (e.key === "Enter" || e.key === " ") {
                    e.preventDefault();
                    toggleTooltip();
                  }
                }}
              >
                <path
                  strokeLinecap="round"
                  strokeLinejoin="round"
                  strokeWidth={2}
                  d="M13 16h-1v-4h-1m1-4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z"
                />
              </svg>
              {showTooltip && (
                <Tooltip targetRef={tooltipRef} position="top">
                  <p className="text-sm text-center leading-relaxed">
                    {description}
                  </p>
                </Tooltip>
              )}
            </div>
          </div>
          <div className="w-full">{children}</div>
        </div>
      );
    }

    return (
      <div className={containerClasses}>
        <div className="mb-2">
          <h3 className={`text-sm font-medium ${disabled ? "opacity-50" : ""}`}>
            {title}
          </h3>
          <p className={`text-sm ${disabled ? "opacity-50" : ""}`}>
            {description}
          </p>
        </div>
        <div className="w-full">{children}</div>
      </div>
    );
  }

  // Horizontal layout (default)
  const horizontalContainerClasses = grouped
    ? "flex items-center justify-between px-4 p-2"
    : "flex items-center justify-between px-4 p-2 rounded-lg border border-mid-gray/20";

  if (descriptionMode === "tooltip") {
    return (
      <div className={horizontalContainerClasses}>
        <div className="max-w-2/3">
          <div className="flex items-center gap-2">
            <h3
              className={`text-sm font-medium ${disabled ? "opacity-50" : ""}`}
            >
              {title}
            </h3>
            <div
              ref={tooltipRef}
              className="relative"
              onMouseEnter={() => setShowTooltip(true)}
              onMouseLeave={() => setShowTooltip(false)}
              onClick={toggleTooltip}
            >
              <svg
                className="w-4 h-4 text-mid-gray cursor-help hover:text-logo-primary transition-colors duration-200 select-none"
                fill="none"
                stroke="currentColor"
                viewBox="0 0 24 24"
                aria-label="More information"
                role="button"
                tabIndex={0}
                onKeyDown={(e) => {
                  if (e.key === "Enter" || e.key === " ") {
                    e.preventDefault();
                    toggleTooltip();
                  }
                }}
              >
                <path
                  strokeLinecap="round"
                  strokeLinejoin="round"
                  strokeWidth={2}
                  d="M13 16h-1v-4h-1m1-4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z"
                />
              </svg>
              {showTooltip && (
                <Tooltip targetRef={tooltipRef} position={tooltipPosition}>
                  <p className="text-sm text-center leading-relaxed">
                    {description}
                  </p>
                </Tooltip>
              )}
            </div>
          </div>
        </div>
        <div className="relative">{children}</div>
      </div>
    );
  }

  return (
    <div className={horizontalContainerClasses}>
      <div className="max-w-2/3">
        <h3 className={`text-sm font-medium ${disabled ? "opacity-50" : ""}`}>
          {title}
        </h3>
        <p className={`text-sm ${disabled ? "opacity-50" : ""}`}>
          {description}
        </p>
      </div>
      <div className="relative">{children}</div>
    </div>
  );
};

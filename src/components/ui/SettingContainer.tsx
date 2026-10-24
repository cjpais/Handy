import React from "react";
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
  const info = (
    <Tooltip
      text={description}
      placement={tooltipPosition === "bottom" ? "below" : "above"}
    >
      <button
        type="button"
        aria-label="More information"
        className="grid cursor-help text-ink-3 transition-colors duration-200 hover:text-logo-primary"
      >
        <svg
          className="w-4 h-4"
          fill="none"
          stroke="currentColor"
          viewBox="0 0 24 24"
          aria-hidden="true"
        >
          <path
            strokeLinecap="round"
            strokeLinejoin="round"
            strokeWidth={2}
            d="M13 16h-1v-4h-1m1-4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z"
          />
        </svg>
      </button>
    </Tooltip>
  );

  const containerClasses = grouped
    ? "px-4 p-2"
    : "px-4 p-2 rounded-lg border border-line";

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
            {info}
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
    ? "flex items-center justify-between min-h-12 px-4 p-2"
    : "flex items-center justify-between min-h-12 px-4 p-2 rounded-lg border border-line";

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
            {info}
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

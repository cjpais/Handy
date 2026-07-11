import React, { useState } from "react";
import { SettingContainer } from "./SettingContainer";

interface TextDisplayProps {
  label: string;
  description: string;
  value: string;
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
  placeholder?: string;
  copyable?: boolean;
  monospace?: boolean;
  onCopy?: (value: string) => void;
}

export const TextDisplay: React.FC<TextDisplayProps> = ({
  label,
  description,
  value,
  descriptionMode = "tooltip",
  grouped = false,
  placeholder = "Not available",
  copyable = false,
  monospace = false,
  onCopy,
}) => {
  const [showCopied, setShowCopied] = useState(false);

  const handleCopy = async () => {
    if (!value || !copyable) return;

    try {
      await navigator.clipboard.writeText(value);
      setShowCopied(true);
      setTimeout(() => setShowCopied(false), 1500);
      if (onCopy) {
        onCopy(value);
      }
    } catch (err) {
      console.error("Failed to copy to clipboard:", err);
    }
  };

  const displayValue = value || placeholder;
  const textClasses = monospace ? "font-mono break-all" : "break-words";

  return (
    <SettingContainer
      title={label}
      description={description}
      descriptionMode={descriptionMode}
      grouped={grouped}
      layout="stacked"
    >
      <div className="flex items-center space-x-2">
        <div className="flex-1 min-w-0">
          <div
            className={`px-2 min-h-8 flex items-center bg-mid-gray/10 border border-mid-gray/80 rounded-md text-xs ${textClasses} ${!value ? "opacity-60" : ""}`}
          >
            {displayValue}
          </div>
        </div>
        {copyable && value && (
          <button
            onClick={handleCopy}
            className="flex items-center justify-center px-2 py-1 w-12 min-h-8 text-xs font-semibold bg-mid-gray/10 hover:bg-logo-primary/10 border border-mid-gray/80 hover:border-logo-primary hover:text-logo-primary rounded-md transition-all duration-150 flex-shrink-0 cursor-pointer"
            title="Copy to clipboard"
          >
            {showCopied ? (
              <div className="flex items-center space-x-1">
                <svg
                  className="w-4 h-4"
                  fill="none"
                  stroke="currentColor"
                  viewBox="0 0 24 24"
                >
                  <path
                    strokeLinecap="round"
                    strokeLinejoin="round"
                    strokeWidth={2}
                    d="M5 13l4 4L19 7"
                  />
                </svg>
              </div>
            ) : (
              "Copy"
            )}
          </button>
        )}
      </div>
    </SettingContainer>
  );
};

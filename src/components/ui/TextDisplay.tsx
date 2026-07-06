import React, { useState } from "react";
import { SettingContainer } from "./SettingContainer";
import { Button } from "./Button";

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
            className={`px-3.5 py-2.5 min-h-[38px] flex items-center bg-orange-off-white border border-stone-mist rounded-inputs text-[13px] ${textClasses} text-charcoal ${!value ? "opacity-50" : ""}`}
          >
            {displayValue}
          </div>
        </div>
        {copyable && value && (
          <Button
            onClick={handleCopy}
            variant="secondary"
            className="w-16 h-[38px] shrink-0"
            title="Copy to clipboard"
          >
            {showCopied ? (
              <svg
                className="w-4 h-4 text-forest-green animate-pulse"
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
            ) : (
              "Copy"
            )}
          </Button>
        )}
      </div>
    </SettingContainer>
  );
};

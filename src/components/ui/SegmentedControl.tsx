import React from "react";

export interface SegmentedControlOption {
  value: string;
  label: string;
}

interface SegmentedControlProps {
  options: SegmentedControlOption[];
  value: string;
  onChange: (value: string) => void;
  disabled?: boolean;
}

export const SegmentedControl: React.FC<SegmentedControlProps> = ({
  options,
  value,
  onChange,
  disabled = false,
}) => {
  return (
    <div
      className={`inline-flex rounded-md border border-mid-gray/80 overflow-hidden ${
        disabled ? "opacity-50 cursor-not-allowed" : ""
      }`}
    >
      {options.map((option, index) => {
        const isSelected = option.value === value;
        const isFirst = index === 0;
        const isLast = index === options.length - 1;

        let borderClasses = "";
        if (!isFirst) {
          borderClasses = "border-l border-mid-gray/80";
        }

        return (
          <button
            key={option.value}
            type="button"
            disabled={disabled}
            onClick={() => !disabled && onChange(option.value)}
            className={`
              px-3 py-1 text-sm font-semibold transition-all duration-150
              ${borderClasses}
              ${isFirst ? "rounded-s-md" : ""}
              ${isLast ? "rounded-e-md" : ""}
              ${
                isSelected
                  ? "bg-logo-primary text-white"
                  : "bg-mid-gray/10 text-text hover:bg-mid-gray/20"
              }
              ${disabled ? "cursor-not-allowed" : "cursor-pointer"}
            `}
          >
            {option.label}
          </button>
        );
      })}
    </div>
  );
};

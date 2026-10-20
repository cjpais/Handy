import React, { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

export interface DropdownOption {
  value: string;
  label: string;
  description?: string;
  disabled?: boolean;
}

interface DropdownProps {
  options: DropdownOption[];
  className?: string;
  menuClassName?: string;
  selectedValue: string | null;
  onSelect: (value: string) => void;
  placeholder?: string;
  disabled?: boolean;
  onOpen?: () => void;
}

export const Dropdown: React.FC<DropdownProps> = ({
  options,
  selectedValue,
  onSelect,
  className = "",
  menuClassName,
  placeholder = "Select an option...",
  disabled = false,
  onOpen,
}) => {
  const { t } = useTranslation();
  const [isOpen, setIsOpen] = useState(false);
  const dropdownRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const handleClickOutside = (event: MouseEvent) => {
      if (
        dropdownRef.current &&
        !dropdownRef.current.contains(event.target as Node)
      ) {
        setIsOpen(false);
      }
    };
    document.addEventListener("mousedown", handleClickOutside);
    return () => document.removeEventListener("mousedown", handleClickOutside);
  }, []);

  const selectedOption = options.find(
    (option) => option.value === selectedValue,
  );

  const handleSelect = (value: string) => {
    onSelect(value);
    setIsOpen(false);
  };

  const handleToggle = () => {
    if (disabled) return;
    if (!isOpen) onOpen?.();
    setIsOpen(!isOpen);
  };

  return (
    <div className={`relative ${className}`} ref={dropdownRef}>
      <button
        type="button"
        className={`px-2 py-[5px] text-sm font-semibold bg-mid-gray/10 border border-mid-gray/80 rounded-md min-w-[200px] w-full text-start grid grid-cols-[1fr_auto] gap-2 items-center transition-all duration-150 v2:h-[34px] v2:py-0 v2:px-[12px] v2:text-small v2:bg-background v2:border-[1.5px] v2:border-line-strong v2:rounded-control ${
          disabled
            ? "opacity-50 cursor-not-allowed"
            : "hover:bg-logo-primary/10 cursor-pointer hover:border-logo-primary v2:hover:bg-background v2:hover:border-plum-edge"
        }`}
        onClick={handleToggle}
        disabled={disabled}
      >
        <span className="truncate">{selectedOption?.label || placeholder}</span>
        <svg
          className={`w-4 h-4 transition-transform duration-200 ${isOpen ? "transform rotate-180" : ""}`}
          fill="none"
          stroke="currentColor"
          viewBox="0 0 24 24"
        >
          <path
            strokeLinecap="round"
            strokeLinejoin="round"
            strokeWidth={2}
            d="M19 9l-7 7-7-7"
          />
        </svg>
      </button>
      {isOpen && !disabled && (
        <div
          className={`absolute top-full mt-1 bg-background border border-mid-gray/80 rounded-md shadow-lg z-50 max-h-60 overflow-y-auto v2:p-1 v2:border-[1.5px] v2:border-line-strong v2:rounded-control ${
            menuClassName ?? "left-0 right-0"
          }`}
        >
          {options.length === 0 ? (
            <div className="px-2 py-1 text-sm text-mid-gray">
              {t("common.noOptionsFound")}
            </div>
          ) : (
            options.map((option) => (
              <button
                key={option.value}
                type="button"
                className={`w-full text-sm text-start hover:bg-logo-primary/10 transition-colors duration-150 v2:text-small v2:rounded-[7px] v2:hover:bg-wash ${
                  option.description ? "px-3 py-2" : "px-2 py-1"
                } ${
                  selectedValue === option.value
                    ? "bg-logo-primary/20 v2:bg-wash"
                    : ""
                } ${option.disabled ? "opacity-50 cursor-not-allowed" : ""}`}
                onClick={() => handleSelect(option.value)}
                disabled={option.disabled}
              >
                <span
                  className={`block whitespace-normal break-words ${
                    option.description || selectedValue === option.value
                      ? "font-semibold"
                      : ""
                  }`}
                >
                  {option.label}
                </span>
                {option.description && (
                  <span className="mt-0.5 block whitespace-normal text-xs font-normal leading-snug text-mid-gray">
                    {option.description}
                  </span>
                )}
              </button>
            ))
          )}
        </div>
      )}
    </div>
  );
};

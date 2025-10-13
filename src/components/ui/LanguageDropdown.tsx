import React, { useState, useRef, useEffect, useMemo } from "react";
import { LANGUAGES } from "../../lib/constants/languages";

interface LanguageDropdownProps {
  value: string | null;
  onChange: (language: string | null) => void;
  disabled?: boolean;
  placeholder?: string;
}

export const LanguageDropdown: React.FC<LanguageDropdownProps> = ({
  value,
  onChange,
  disabled = false,
  placeholder = "Auto",
}) => {
  const [isOpen, setIsOpen] = useState(false);
  const [searchQuery, setSearchQuery] = useState("");
  const dropdownRef = useRef<HTMLDivElement>(null);
  const searchInputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    const handleClickOutside = (event: MouseEvent) => {
      if (
        dropdownRef.current &&
        !dropdownRef.current.contains(event.target as Node)
      ) {
        setIsOpen(false);
        setSearchQuery("");
      }
    };

    document.addEventListener("mousedown", handleClickOutside);
    return () => {
      document.removeEventListener("mousedown", handleClickOutside);
    };
  }, []);

  useEffect(() => {
    if (isOpen && searchInputRef.current) {
      searchInputRef.current.focus();
    }
  }, [isOpen]);

  const filteredLanguages = useMemo(
    () =>
      LANGUAGES.filter((language) =>
        language.label.toLowerCase().includes(searchQuery.toLowerCase()),
      ),
    [searchQuery],
  );

  const selectedLanguageName =
    LANGUAGES.find((lang) => lang.value === value)?.label || "Auto Detect";

  const handleLanguageSelect = (languageCode: string) => {
    onChange(languageCode === "auto" ? null : languageCode);
    setIsOpen(false);
    setSearchQuery("");
  };

  const handleToggle = () => {
    if (disabled) return;
    setIsOpen(!isOpen);
  };

  const handleSearchChange = (event: React.ChangeEvent<HTMLInputElement>) => {
    setSearchQuery(event.target.value);
  };

  const handleKeyDown = (event: React.KeyboardEvent<HTMLInputElement>) => {
    if (event.key === "Enter" && filteredLanguages.length > 0) {
      // Select first filtered language on Enter
      handleLanguageSelect(filteredLanguages[0].value);
    } else if (event.key === "Escape") {
      setIsOpen(false);
      setSearchQuery("");
    }
  };

  return (
    <div className="relative" ref={dropdownRef}>
      <button
        type="button"
        className={`px-2 py-1 text-sm font-semibold bg-mid-gray/10 border border-mid-gray/80 rounded min-w-[200px] text-left flex items-center justify-between transition-all duration-150 ${
          disabled
            ? "opacity-50 cursor-not-allowed"
            : "hover:bg-logo-primary/10 cursor-pointer hover:border-logo-primary"
        }`}
        onClick={handleToggle}
        disabled={disabled}
      >
        <span className="truncate">{selectedLanguageName}</span>
        <svg
          className={`w-4 h-4 ml-2 transition-transform duration-200 flex-shrink-0 ${
            isOpen ? "transform rotate-180" : ""
          }`}
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
        <div className="absolute top-full left-0 right-0 mt-1 bg-background border border-mid-gray/80 rounded shadow-lg z-50 max-h-60 overflow-hidden">
          {/* Search input */}
          <div className="p-2 border-b border-mid-gray/80">
            <input
              ref={searchInputRef}
              type="text"
              value={searchQuery}
              onChange={handleSearchChange}
              onKeyDown={handleKeyDown}
              placeholder="Search languages..."
              className="w-full px-2 py-1 text-sm bg-mid-gray/10 border border-mid-gray/40 rounded focus:outline-none focus:ring-1 focus:ring-logo-primary focus:border-logo-primary"
            />
          </div>

          <div className="max-h-48 overflow-y-auto">
            {filteredLanguages.length === 0 ? (
              <div className="px-2 py-2 text-sm text-mid-gray text-center">
                No languages found
              </div>
            ) : (
              filteredLanguages.map((language) => {
                const isSelected = value === language.value || (value === null && language.value === "auto");
                return (
                  <button
                    key={language.value}
                    type="button"
                    className={`w-full px-2 py-1 text-sm text-left hover:bg-logo-primary/10 transition-colors duration-150 ${
                      isSelected
                        ? "bg-logo-primary/20 text-logo-primary font-semibold"
                        : ""
                    }`}
                    onClick={() => handleLanguageSelect(language.value)}
                  >
                    <div className="flex items-center justify-between">
                      <span className="truncate">{language.label}</span>
                    </div>
                  </button>
                );
              })
            )}
          </div>
        </div>
      )}
    </div>
  );
};

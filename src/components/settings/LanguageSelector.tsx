import React, { useState, useRef, useEffect, useMemo } from "react";
import { listen } from "@tauri-apps/api/event";
import { SettingContainer } from "../ui/SettingContainer";
import { ResetButton } from "../ui/ResetButton";
import { useSettings } from "../../hooks/useSettings";
import { useModels } from "../../hooks/useModels";
import { LANGUAGES } from "../../lib/constants/languages";

interface LanguageSelectorProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

const unsupportedModels = ["parakeet-tdt-0.6b-v2", "parakeet-tdt-0.6b-v3"];

export const LanguageSelector: React.FC<LanguageSelectorProps> = ({
  descriptionMode = "tooltip",
  grouped = false,
}) => {
  const { getSetting, updateSetting, resetSetting, isUpdating } = useSettings();
  const { currentModel, loadCurrentModel } = useModels();
  const [isOpen, setIsOpen] = useState(false);
  const [searchQuery, setSearchQuery] = useState("");
  const dropdownRef = useRef<HTMLDivElement>(null);
  const searchInputRef = useRef<HTMLInputElement>(null);

  const selectedLanguage = getSetting("selected_language") || "auto";
  const isUnsupported = unsupportedModels.includes(currentModel);

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

  // Listen for model state changes to update UI reactively
  useEffect(() => {
    const modelStateUnlisten = listen("model-state-changed", () => {
      loadCurrentModel();
    });

    return () => {
      modelStateUnlisten.then((fn) => fn());
    };
  }, [loadCurrentModel]);

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

  const selectedLanguageName = isUnsupported
    ? "Auto"
    : LANGUAGES.find((lang) => lang.value === selectedLanguage)?.label ||
      "Auto";

  const handleLanguageSelect = async (languageCode: string) => {
    await updateSetting("selected_language", languageCode);
    setIsOpen(false);
    setSearchQuery("");
  };

  const handleReset = async () => {
    await resetSetting("selected_language");
  };

  const handleToggle = () => {
    if (isUpdating("selected_language") || isUnsupported) return;
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
    <SettingContainer
      title="Language"
      description={
        isUnsupported
          ? "Parakeet model automatically detects the language. No manual selection is needed."
          : "Select the language for speech recognition. Auto will automatically determine the language, while selecting a specific language can improve accuracy for that language."
      }
      descriptionMode={descriptionMode}
      grouped={grouped}
      disabled={isUnsupported}
    >
      <div className="flex items-center space-x-1">
        <div className="relative" ref={dropdownRef}>
          <button
            type="button"
            className={`px-2 py-1 text-sm font-semibold bg-mid-gray/10 border border-mid-gray/80 rounded min-w-[200px] text-left flex items-center justify-between transition-all duration-150 ${
              isUpdating("selected_language") || isUnsupported
                ? "opacity-50 cursor-not-allowed"
                : "hover:bg-logo-primary/10 cursor-pointer hover:border-logo-primary"
            }`}
            onClick={handleToggle}
            disabled={isUpdating("selected_language") || isUnsupported}
          >
            <span className="truncate">{selectedLanguageName}</span>
            <svg
              className={`w-4 h-4 ml-2 transition-transform duration-200 ${
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

          {isOpen && !isUpdating("selected_language") && !isUnsupported && (
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
                  filteredLanguages.map((language) => (
                    <button
                      key={language.value}
                      type="button"
                      className={`w-full px-2 py-1 text-sm text-left hover:bg-logo-primary/10 transition-colors duration-150 ${
                        selectedLanguage === language.value
                          ? "bg-logo-primary/20 text-logo-primary font-semibold"
                          : ""
                      }`}
                      onClick={() => handleLanguageSelect(language.value)}
                    >
                      <div className="flex items-center justify-between">
                        <span className="truncate">{language.label}</span>
                      </div>
                    </button>
                  ))
                )}
              </div>
            </div>
          )}
        </div>
        <ResetButton
          onClick={handleReset}
          disabled={isUpdating("selected_language") || isUnsupported}
        />
      </div>
      {isUpdating("selected_language") && (
        <div className="absolute inset-0 bg-mid-gray/10 rounded flex items-center justify-center">
          <div className="w-4 h-4 border-2 border-logo-primary border-t-transparent rounded-full animate-spin"></div>
        </div>
      )}
    </SettingContainer>
  );
};

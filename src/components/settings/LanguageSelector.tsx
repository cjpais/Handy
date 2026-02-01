import React, { useState, useRef, useEffect } from "react";
import { SettingContainer } from "../ui/SettingContainer";
import ResetIcon from "../icons/ResetIcon";
import { useSettings } from "../../hooks/useSettings";

interface LanguageSelectorProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

const LANGUAGES = [
  { code: "auto", name: "Auto" },
  { code: "en", name: "English" },
  { code: "zh", name: "Chinese (Auto)" },
  { code: "zh-TW", name: "Chinese Traditional" },
  { code: "zh-CN", name: "Chinese Simplified" },
  { code: "hi", name: "Hindi" },
  { code: "es", name: "Spanish" },
  { code: "fr", name: "French" },
  { code: "ar", name: "Arabic" },
  { code: "pt", name: "Portuguese" },
  { code: "ru", name: "Russian" },
  { code: "ja", name: "Japanese" },
  { code: "de", name: "German" },
  { code: "ko", name: "Korean" },
  { code: "it", name: "Italian" },
  { code: "tr", name: "Turkish" },
  { code: "vi", name: "Vietnamese" },
  { code: "pl", name: "Polish" },
  { code: "nl", name: "Dutch" },
  { code: "uk", name: "Ukrainian" },
  { code: "fa", name: "Persian" },
  { code: "th", name: "Thai" },
  { code: "ro", name: "Romanian" },
  { code: "el", name: "Greek" },
  { code: "cs", name: "Czech" },
  { code: "sv", name: "Swedish" },
  { code: "hu", name: "Hungarian" },
  { code: "he", name: "Hebrew" },
  { code: "da", name: "Danish" },
  { code: "fi", name: "Finnish" },
  { code: "no", name: "Norwegian" },
  { code: "sk", name: "Slovak" },
  { code: "bg", name: "Bulgarian" },
  { code: "hr", name: "Croatian" },
  { code: "lt", name: "Lithuanian" },
  { code: "sl", name: "Slovenian" },
  { code: "lv", name: "Latvian" },
  { code: "et", name: "Estonian" },
  { code: "sr", name: "Serbian" },
  { code: "is", name: "Icelandic" },
  { code: "id", name: "Indonesian" },
];

export const LanguageSelector: React.FC<LanguageSelectorProps> = ({
  descriptionMode = "tooltip",
  grouped = false,
}) => {
  const { getSetting, updateSetting, resetSetting, isUpdating } = useSettings();
  const [isOpen, setIsOpen] = useState(false);
  const [searchQuery, setSearchQuery] = useState("");
  const dropdownRef = useRef<HTMLDivElement>(null);
  const searchInputRef = useRef<HTMLInputElement>(null);

  const selectedLanguage = getSetting("selected_language") || "auto";

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

  const filteredLanguages = LANGUAGES.filter((language) =>
    language.name.toLowerCase().includes(searchQuery.toLowerCase()),
  );

  const selectedLanguageName =
    LANGUAGES.find((lang) => lang.code === selectedLanguage)?.name || "Auto";

  const handleLanguageSelect = async (languageCode: string) => {
    await updateSetting("selected_language", languageCode);
    setIsOpen(false);
    setSearchQuery("");
  };

  const handleReset = async () => {
    await resetSetting("selected_language");
  };

  const handleToggle = () => {
    if (isUpdating("selected_language")) return;
    setIsOpen(!isOpen);
  };

  const handleSearchChange = (event: React.ChangeEvent<HTMLInputElement>) => {
    setSearchQuery(event.target.value);
  };

  const handleKeyDown = (event: React.KeyboardEvent<HTMLInputElement>) => {
    if (event.key === "Enter" && filteredLanguages.length > 0) {
      // Select first filtered language on Enter
      handleLanguageSelect(filteredLanguages[0].code);
    } else if (event.key === "Escape") {
      setIsOpen(false);
      setSearchQuery("");
    }
  };

  return (
    <SettingContainer
      title="Language"
      description="Select the language for speech recognition. Auto will automatically determine the language. For Chinese speakers, select Traditional or Simplified to ensure correct character output."
      descriptionMode={descriptionMode}
      grouped={grouped}
    >
      <div className="flex items-center space-x-1">
        <div className="relative" ref={dropdownRef}>
          <button
            type="button"
            className={`px-2 py-1 text-sm font-semibold bg-mid-gray/10 border border-mid-gray/80 rounded min-w-[200px] text-left flex items-center justify-between transition-all duration-150 ${
              isUpdating("selected_language")
                ? "opacity-50 cursor-not-allowed"
                : "hover:bg-logo-primary/10 cursor-pointer hover:border-logo-primary"
            }`}
            onClick={handleToggle}
            disabled={isUpdating("selected_language")}
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

          {isOpen && !isUpdating("selected_language") && (
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
                      key={language.code}
                      type="button"
                      className={`w-full px-2 py-1 text-sm text-left hover:bg-logo-primary/10 transition-colors duration-150 ${
                        selectedLanguage === language.code
                          ? "bg-logo-primary/20 text-logo-primary font-semibold"
                          : ""
                      }`}
                      onClick={() => handleLanguageSelect(language.code)}
                    >
                      <div className="flex items-center justify-between">
                        <span className="truncate">{language.name}</span>
                      </div>
                    </button>
                  ))
                )}
              </div>
            </div>
          )}
        </div>
        <button
          className="px-2 py-1 hover:bg-logo-primary/30 active:bg-logo-primary/50 active:scale-95 rounded fill-text hover:cursor-pointer hover:border-logo-primary border border-transparent transition-all duration-150"
          onClick={handleReset}
          disabled={isUpdating("selected_language")}
        >
          <ResetIcon className="" />
        </button>
      </div>
      {isUpdating("selected_language") && (
        <div className="absolute inset-0 bg-mid-gray/10 rounded flex items-center justify-center">
          <div className="w-4 h-4 border-2 border-logo-primary border-t-transparent rounded-full animate-spin"></div>
        </div>
      )}
    </SettingContainer>
  );
};

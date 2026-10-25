import React, { useState, useRef, useEffect, useMemo } from "react";
import { useTranslation } from "react-i18next";
import { SettingContainer } from "../ui/SettingContainer";
import { ResetButton } from "../ui/ResetButton";
import { useSettings } from "../../hooks/useSettings";
import { languageIdentity } from "@handy-computer/models";
import {
  LANGUAGES,
  languageName,
  languagesOf,
  searchLanguages,
  supportsLanguage,
} from "../../lib/constants/languages";

interface LanguageSelectorProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
  supportedLanguages?: string[];
  // Whether the model can auto-detect language. Gates the "Auto" option:
  // must-pick models (no detection) omit it and force a concrete choice.
  supportsLanguageDetection?: boolean;
}

// Mirrors `effective_language` in src-tauri/src/managers/model.rs, but returns
// the picker's language identity (`jv`), not the model's concrete code (`jw`).
const effectiveLanguage = (
  intent: string,
  preferred: string[],
  supported: string[],
  supportsDetection: boolean,
): string => {
  const wanted = languageIdentity(intent);
  if (supported.length === 0) return wanted;
  if (wanted !== "auto" && supportsLanguage(supported, wanted)) return wanted;
  if (supportsDetection) return "auto";
  if (preferred.length === 1 && supportsLanguage(supported, preferred[0]))
    return languageIdentity(preferred[0]);
  if (supportsLanguage(supported, "en")) return "en";
  return languageIdentity(supported[0]);
};

export const LanguageSelector: React.FC<LanguageSelectorProps> = ({
  descriptionMode = "tooltip",
  grouped = false,
  supportedLanguages,
  supportsLanguageDetection = true,
}) => {
  const { t, i18n } = useTranslation();
  const { getSetting, updateSetting, resetSetting, isUpdating } = useSettings();
  const [isOpen, setIsOpen] = useState(false);
  const [searchQuery, setSearchQuery] = useState("");
  const dropdownRef = useRef<HTMLDivElement>(null);
  const searchInputRef = useRef<HTMLInputElement>(null);

  // The persisted *intent* (auto | code). What's actually used/shown is the
  // effective value resolved against the current model's capabilities.
  const intent = getSetting("selected_language") || "auto";
  const selectedLanguage = effectiveLanguage(
    intent,
    getSetting("spoken_languages") ?? [],
    supportedLanguages ?? [],
    supportsLanguageDetection,
  );

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

  // The current model's own languages; every language before one is loaded.
  const hasModelLanguages = !!supportedLanguages?.length;
  // The languages they speak lead, in the order they added them.
  const spoken = getSetting("spoken_languages");
  const availableLanguages = useMemo(() => {
    const all = supportedLanguages?.length
      ? languagesOf(supportedLanguages)
      : LANGUAGES;
    const rank = (code: string) => {
      const i = spoken?.indexOf(code) ?? -1;
      return i < 0 ? Infinity : i;
    };
    return [...all].sort((a, b) => {
      const ra = rank(a.code);
      const rb = rank(b.code);
      return ra === rb ? 0 : ra < rb ? -1 : 1;
    });
  }, [supportedLanguages, spoken]);

  const ui = i18n.language;
  const autoLabel = t("settings.general.language.auto");
  const filteredLanguages = useMemo(() => {
    const showAuto =
      (!hasModelLanguages || supportsLanguageDetection) &&
      autoLabel.toLowerCase().includes(searchQuery.trim().toLowerCase());
    return [
      ...(showAuto ? [{ value: "auto", label: autoLabel }] : []),
      ...searchLanguages(searchQuery, ui, availableLanguages).map((l) => ({
        value: l.code,
        label: languageName(l.code, ui),
      })),
    ];
  }, [
    searchQuery,
    ui,
    autoLabel,
    availableLanguages,
    hasModelLanguages,
    supportsLanguageDetection,
  ]);

  const selectedLanguageName =
    selectedLanguage === "auto"
      ? autoLabel
      : languageName(selectedLanguage, ui);

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
      handleLanguageSelect(filteredLanguages[0].value);
    } else if (event.key === "Escape") {
      setIsOpen(false);
      setSearchQuery("");
    }
  };

  return (
    <SettingContainer
      title={t("settings.general.language.title")}
      description={t("settings.general.language.description")}
      descriptionMode={descriptionMode}
      grouped={grouped}
    >
      <div className="flex items-center space-x-1">
        <div className="relative" ref={dropdownRef}>
          <button
            type="button"
            className={`px-2 py-1 text-sm font-semibold bg-wash border border-line-strong rounded min-w-[200px] text-start flex items-center justify-between transition-all duration-150 ${
              isUpdating("selected_language")
                ? "opacity-50 cursor-not-allowed"
                : "hover:bg-logo-primary/10 cursor-pointer hover:border-logo-primary"
            }`}
            onClick={handleToggle}
            disabled={isUpdating("selected_language")}
          >
            <span className="truncate">{selectedLanguageName}</span>
            <svg
              className={`w-4 h-4 ms-2 transition-transform duration-200 ${
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
            <div className="absolute top-full left-0 right-0 mt-1 bg-background border border-line-strong rounded shadow-lg z-50 max-h-60 overflow-hidden">
              {/* Search input */}
              <div className="p-2 border-b border-line-strong">
                <input
                  ref={searchInputRef}
                  type="text"
                  value={searchQuery}
                  onChange={handleSearchChange}
                  onKeyDown={handleKeyDown}
                  placeholder={t("settings.general.language.searchPlaceholder")}
                  className="w-full px-2 py-1 text-sm bg-wash border border-line rounded focus:outline-none focus:ring-1 focus:ring-logo-primary focus:border-logo-primary"
                />
              </div>

              <div className="max-h-48 overflow-y-auto">
                {filteredLanguages.length === 0 ? (
                  <div className="px-2 py-2 text-sm text-ink-3 text-center">
                    {t("settings.general.language.noResults")}
                  </div>
                ) : (
                  filteredLanguages.map((language) => (
                    <button
                      key={language.value}
                      type="button"
                      className={`w-full px-2 py-1 text-sm text-start hover:bg-logo-primary/10 transition-colors duration-150 ${
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
          disabled={isUpdating("selected_language")}
        />
      </div>
      {isUpdating("selected_language") && (
        <div className="absolute inset-0 bg-wash rounded flex items-center justify-center">
          <div className="w-4 h-4 border-2 border-logo-primary border-t-transparent rounded-full animate-spin"></div>
        </div>
      )}
    </SettingContainer>
  );
};

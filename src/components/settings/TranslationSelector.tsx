import React, { useState, useRef, useEffect, useMemo } from "react";
import { useTranslation } from "react-i18next";
import { SettingContainer } from "../ui/SettingContainer";
import { ResetButton } from "../ui/ResetButton";
import { Alert } from "../ui/Alert";
import { useSettings } from "../../hooks/useSettings";
import { useModelStore } from "../../stores/modelStore";
import {
  getLanguageLabel,
  LANGUAGES,
  SELECTABLE_LANGUAGES,
} from "../../lib/constants/languages";
import type { ModelInfo } from "@/bindings";

interface TranslationSelectorProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const TranslationSelector: React.FC<TranslationSelectorProps> = ({
  descriptionMode = "tooltip",
  grouped = false,
}) => {
  const { t } = useTranslation();
  const { getSetting, updateSetting, resetSetting, isUpdating } = useSettings();
  const { currentModel, models } = useModelStore();
  const [isOpen, setIsOpen] = useState(false);
  const [searchQuery, setSearchQuery] = useState("");
  const dropdownRef = useRef<HTMLDivElement>(null);
  const searchInputRef = useRef<HTMLInputElement>(null);

  const currentModelInfo = models.find((m: ModelInfo) => m.id === currentModel);
  const modelSupportsTranslation =
    currentModelInfo?.supports_translation ?? false;

  const translationTarget = getSetting("translation_target_language") || "none";

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

  const targetLanguages = useMemo(() => {
    const baseList = SELECTABLE_LANGUAGES.filter(
      (lang) => lang.value !== "auto",
    );
    return [
      { value: "none", label: t("settings.general.translation.disabled") },
      ...baseList,
    ];
  }, [t]);

  const filteredLanguages = useMemo(
    () =>
      targetLanguages.filter((language) =>
        language.label.toLowerCase().includes(searchQuery.toLowerCase()),
      ),
    [searchQuery, targetLanguages],
  );

  const selectedLanguageName =
    translationTarget === "none"
      ? t("settings.general.translation.disabled")
      : getLanguageLabel(translationTarget) || translationTarget;

  const handleLanguageSelect = async (languageCode: string) => {
    await updateSetting("translation_target_language", languageCode);
    setIsOpen(false);
    setSearchQuery("");
  };

  const handleReset = async () => {
    await resetSetting("translation_target_language");
  };

  const handleToggle = () => {
    if (isUpdating("translation_target_language")) return;
    setIsOpen(!isOpen);
  };

  const handleSearchChange = (event: React.ChangeEvent<HTMLInputElement>) => {
    setSearchQuery(event.target.value);
  };

  const handleKeyDown = (event: React.KeyboardEvent<HTMLInputElement>) => {
    if (event.key === "Enter" && filteredLanguages.length > 0) {
      handleLanguageSelect(filteredLanguages[0].value);
    } else if (event.key === "Escape") {
      setIsOpen(false);
      setSearchQuery("");
    }
  };

  // Check if translation requires LLM post-processing and if it's configured
  const requiresLlm =
    translationTarget !== "none" &&
    !(translationTarget === "en" && modelSupportsTranslation);

  const providerId = getSetting("post_process_provider_id") || "";
  const apiKeys = getSetting("post_process_api_keys") || {};
  const activeKey = providerId ? apiKeys[providerId] : "";
  const isLlmConfigured =
    providerId === "apple_intelligence" ||
    (activeKey && activeKey.trim() !== "");

  const showModelWarning =
    translationTarget === "en" && !modelSupportsTranslation && !isLlmConfigured;

  const showLlmWarning = requiresLlm && !isLlmConfigured && !showModelWarning;

  return (
    <div className="flex flex-col w-full">
      <SettingContainer
        title={t("settings.general.translation.label")}
        description={t("settings.general.translation.description")}
        descriptionMode={descriptionMode}
        grouped={grouped}
      >
        <div className="flex items-center space-x-1">
          <div className="relative" ref={dropdownRef}>
            <button
              type="button"
              className={`px-2 py-1 text-sm font-semibold bg-mid-gray/10 border border-mid-gray/80 rounded min-w-[200px] text-start flex items-center justify-between transition-all duration-150 ${
                isUpdating("translation_target_language")
                  ? "opacity-50 cursor-not-allowed"
                  : "hover:bg-logo-primary/10 cursor-pointer hover:border-logo-primary"
              }`}
              onClick={handleToggle}
              disabled={isUpdating("translation_target_language")}
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

            {isOpen && !isUpdating("translation_target_language") && (
              <div className="absolute top-full left-0 right-0 mt-1 bg-background border border-mid-gray/80 rounded shadow-lg z-50 max-h-60 overflow-hidden">
                <div className="p-2 border-b border-mid-gray/80">
                  <input
                    ref={searchInputRef}
                    type="text"
                    value={searchQuery}
                    onChange={handleSearchChange}
                    onKeyDown={handleKeyDown}
                    placeholder={t(
                      "settings.general.language.searchPlaceholder",
                    )}
                    className="w-full px-2 py-1 text-sm bg-mid-gray/10 border border-mid-gray/40 rounded focus:outline-none focus:ring-1 focus:ring-logo-primary focus:border-logo-primary"
                  />
                </div>

                <div className="max-h-48 overflow-y-auto">
                  {filteredLanguages.length === 0 ? (
                    <div className="px-2 py-2 text-sm text-mid-gray text-center">
                      {t("settings.general.language.noResults")}
                    </div>
                  ) : (
                    filteredLanguages.map((language) => (
                      <button
                        key={language.value}
                        type="button"
                        className={`w-full px-2 py-1 text-sm text-start hover:bg-logo-primary/10 transition-colors duration-150 ${
                          translationTarget === language.value
                            ? "bg-logo-primary/20 text-logo-primary font-semibold"
                            : "text-text"
                        }`}
                        onClick={() => handleLanguageSelect(language.value)}
                      >
                        {language.label}
                      </button>
                    ))
                  )}
                </div>
              </div>
            )}
          </div>

          {translationTarget !== "none" && (
            <ResetButton
              onClick={handleReset}
              disabled={isUpdating("translation_target_language")}
              ariaLabel={t("settings.general.language.reset")}
            />
          )}
        </div>
      </SettingContainer>

      {showModelWarning && (
        <div className="mt-2 px-4 pb-2">
          <Alert variant="warning" contained>
            {t("settings.general.translation.modelWarning")}
          </Alert>
        </div>
      )}

      {showLlmWarning && (
        <div className="mt-2 px-4 pb-2">
          <Alert variant="warning" contained>
            {t("settings.general.translation.warning")}
          </Alert>
        </div>
      )}
    </div>
  );
};

import React, { useState, useRef, useEffect, useMemo } from "react";
import { useTranslation } from "react-i18next";
import { listen } from "@tauri-apps/api/event";
import { SettingContainer } from "../ui/SettingContainer";
import { ResetButton } from "../ui/ResetButton";
import { InfoTooltip } from "../ui/InfoTooltip";
import { Tooltip } from "../ui/Tooltip";
import { LanguageDetectorDownload } from "./LanguageDetectorDownload";
import { useSettings } from "../../hooks/useSettings";
import { useLanguageDetector } from "../../hooks/useLanguageDetector";
import { languageIdentity } from "@handy-computer/models";
import {
  commands,
  type AutoUnavailableReason,
  type LanguageStatus,
} from "@/bindings";
import {
  LANGUAGES,
  languageList,
  languageName,
  languagesOf,
  searchLanguages,
} from "../../lib/constants/languages";

interface LanguageSelectorProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
  /** The model whose language status is shown. */
  modelId: string;
  supportedLanguages?: string[];
}

export const LanguageSelector: React.FC<LanguageSelectorProps> = ({
  descriptionMode = "tooltip",
  grouped = false,
  modelId,
  supportedLanguages,
}) => {
  const { t, i18n } = useTranslation();
  const { getSetting, updateSetting, resetSetting, isUpdating } = useSettings();
  const [isOpen, setIsOpen] = useState(false);
  const [searchQuery, setSearchQuery] = useState("");
  const dropdownRef = useRef<HTMLDivElement>(null);
  const searchInputRef = useRef<HTMLInputElement>(null);

  // The persisted *intent* (auto | code). What's shown is what transcription
  // will actually use, which the backend resolves from the same policy.
  const intent = getSetting("selected_language") || "auto";
  const spoken = getSetting("spoken_languages");
  const detectionMode = getSetting("language_detection");
  const spokenKey = spoken?.join(",");
  const [status, setStatus] = useState<LanguageStatus | null>(null);
  const detector = useLanguageDetector();

  useEffect(() => {
    let cancelled = false;
    const refresh = () =>
      commands.getLanguageStatus(modelId).then((next) => {
        if (!cancelled) setStatus(next);
      });
    refresh();
    // The language detector arriving changes what Auto does.
    const unlisten = listen("model-download-complete", refresh);
    return () => {
      cancelled = true;
      unlisten.then((stop) => stop());
    };
  }, [modelId, intent, spokenKey, detectionMode]);

  const selectedLanguage =
    status === null
      ? languageIdentity(intent)
      : status.kind === "auto"
        ? "auto"
        : status.kind === "detector_missing"
          ? languageIdentity(status.fallback)
          : languageIdentity(status.language);

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

  // The languages they speak lead, in the order they added them.
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
  const autoUnavailableNote = (reason: AutoUnavailableReason): string => {
    switch (reason.kind) {
      case "one_language":
        return t("settings.general.language.autoUnavailable.oneLanguage");
      case "undetectable":
        return t("settings.general.language.autoUnavailable.undetectable", {
          languages: languageList(reason.languages.map(languageIdentity), ui),
        });
      case "no_match":
        return t("settings.general.language.autoUnavailable.noMatch");
    }
  };
  // Auto is always listed; when it can't work here it stays visible, muted,
  // with the reason in a tooltip, so it's clear why it isn't in use. Without
  // the detector it's offered as a download instead.
  const autoNote =
    status?.kind === "auto_unavailable"
      ? autoUnavailableNote(status.reason)
      : status?.kind === "detector_missing"
        ? t("settings.general.language.detectorMissing")
        : undefined;
  const filteredLanguages = useMemo(() => {
    const showAuto = autoLabel
      .toLowerCase()
      .includes(searchQuery.trim().toLowerCase());
    return [
      ...(showAuto
        ? [
            {
              value: "auto",
              label: autoLabel,
              disabled: autoNote !== undefined,
              note: autoNote,
            },
          ]
        : []),
      ...searchLanguages(searchQuery, ui, availableLanguages).map((l) => ({
        value: l.code,
        label: languageName(l.code, ui),
        disabled: false,
        note: undefined,
      })),
    ];
  }, [searchQuery, ui, autoLabel, autoNote, availableLanguages]);

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
    const first = filteredLanguages.find((language) => !language.disabled);
    if (event.key === "Enter" && first) {
      // Select first selectable filtered language on Enter
      handleLanguageSelect(first.value);
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
                  filteredLanguages.map((language) =>
                    language.value === "auto" &&
                    status?.kind === "detector_missing" ? (
                      detector.downloading ? (
                        <div
                          key={language.value}
                          className="flex items-center justify-between gap-2 px-2 py-1 text-sm text-ink-3"
                        >
                          <span className="truncate">{language.label}</span>
                          <LanguageDetectorDownload
                            downloading
                            percent={detector.percent}
                            onCancel={detector.cancel}
                          />
                        </div>
                      ) : (
                        // Like a model not downloaded yet: choosing it downloads it.
                        <Tooltip
                          key={language.value}
                          title={t(
                            "settings.general.language.detectorMissingTitle",
                          )}
                          text={language.note}
                          className="w-full"
                        >
                          <button
                            type="button"
                            onClick={detector.download}
                            className="group flex w-full items-center justify-between gap-2 px-2 py-1 text-sm text-ink-3 transition-colors duration-150 hover:bg-logo-primary/10 hover:text-text"
                          >
                            <span className="truncate">{language.label}</span>
                            <LanguageDetectorDownload
                              downloading={false}
                              percent={0}
                            />
                          </button>
                        </Tooltip>
                      )
                    ) : language.disabled ? (
                      <div
                        key={language.value}
                        className="flex items-center justify-between gap-2 px-2 py-1 text-sm text-ink-3"
                      >
                        <span className="truncate">{language.label}</span>
                        {language.note && (
                          <InfoTooltip
                            title={t(
                              "settings.general.language.autoUnavailable.title",
                            )}
                            text={language.note}
                          />
                        )}
                      </div>
                    ) : (
                      <button
                        key={language.value}
                        type="button"
                        className={`w-full px-2 py-1 text-sm text-start transition-colors duration-150 ${
                          selectedLanguage === language.value
                            ? "bg-logo-primary/20 text-logo-primary font-semibold"
                            : "hover:bg-logo-primary/10"
                        }`}
                        onClick={() => handleLanguageSelect(language.value)}
                      >
                        <span className="block truncate">{language.label}</span>
                      </button>
                    ),
                  )
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

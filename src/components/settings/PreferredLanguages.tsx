import React, { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { SettingContainer } from "../ui/SettingContainer";
import {
  LanguageOptions,
  LanguageSearchField,
  useLanguagePicker,
} from "../shared/LanguagePicker";
import { suggestedLanguages } from "@/lib/utils/languageSuggestions";
import { isWeakLanguage } from "@/lib/models/catalog";
import { useSettings } from "../../hooks/useSettings";
import { languageList } from "../../lib/constants/languages";

interface PreferredLanguagesProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

/** Edits `spoken_languages`; leaves the model's own Language as is. */
export const PreferredLanguages: React.FC<PreferredLanguagesProps> = ({
  descriptionMode = "tooltip",
  grouped = false,
}) => {
  const { t, i18n } = useTranslation();
  const { getSetting, updateSetting } = useSettings();
  const languages = getSetting("spoken_languages") ?? [];
  const [focused, setFocused] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);
  const gridRef = useRef<HTMLDivElement>(null);

  const ui = i18n.language;

  // Nothing picked yet (onboarded before this setting): suggest their language.
  const suggestion = languages.length ? null : suggestedLanguages()[0];
  const picker = useLanguagePicker({
    languages,
    onChange: (next) => {
      updateSetting("spoken_languages", next);
      gridRef.current?.scrollTo({ top: 0 });
    },
    // At least one language, once there is one.
    keepOne: true,
    suggestion,
  });
  const { available, inputRef } = picker;
  const open = focused || !languages.length;

  // A closed grid is out of the tab order (React 18 has no `inert` prop).
  const drawerRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    drawerRef.current?.toggleAttribute("inert", !open);
  }, [open]);

  // The grid's foot fades only when there's more to scroll to.
  const [overflows, setOverflows] = useState(false);
  useEffect(() => {
    const grid = gridRef.current;
    if (grid) setOverflows(grid.scrollHeight > grid.clientHeight + 1);
  }, [available.length, open]);

  const onKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (!picker.onKeyDown(e) && e.key === "Escape") inputRef.current?.blur();
  };

  const weak = languages.filter(isWeakLanguage);

  return (
    <SettingContainer
      title={t("settings.general.preferredLanguages.title")}
      description={t("settings.general.preferredLanguages.description")}
      descriptionMode={descriptionMode}
      grouped={grouped}
      layout="stacked"
    >
      <div
        ref={rootRef}
        onFocus={() => setFocused(true)}
        onBlur={(e) => {
          if (rootRef.current?.contains(e.relatedTarget as Node)) return;
          setFocused(false);
          picker.setQuery("");
        }}
        className={`mt-[2px] overflow-hidden rounded-card border-[1.5px] bg-background transition-colors duration-200 ${
          focused ? "border-ink-3" : "border-line-strong"
        }`}
      >
        <LanguageSearchField
          picker={picker}
          inputClassName="min-w-[140px] text-small"
          onKeyDown={onKeyDown}
          aria-expanded={open}
        />

        {/* The grid opens under the chips: its height eases from nothing. */}
        <div
          className={`grid transition-[grid-template-rows] duration-300 ease-glide motion-reduce:transition-none ${
            open ? "grid-rows-[1fr]" : "grid-rows-[0fr]"
          }`}
        >
          <div ref={drawerRef} className="min-h-0">
            <div
              ref={gridRef}
              // Keep focus in the search: WebKit doesn't focus a pressed button,
              // so the blur would close the grid before the click lands.
              onMouseDown={(e) => {
                if ((e.target as HTMLElement).closest("button"))
                  e.preventDefault();
              }}
              role="group"
              aria-label={t("languagePicker.search")}
              className={`grid max-h-[214px] grid-cols-[repeat(auto-fill,minmax(150px,1fr))] content-start gap-[2px] overflow-y-auto border-t border-line p-[6px] ${
                overflows
                  ? "pb-[14px] [mask-image:linear-gradient(to_bottom,black_calc(100%-36px),transparent_calc(100%-4px))]"
                  : ""
              }`}
            >
              <LanguageOptions
                picker={picker}
                highlightFirst={!!suggestion}
                noMatch={t("settings.general.preferredLanguages.noMatch", {
                  query: picker.query.trim(),
                })}
                emptyClassName="p-[20px]"
              />
            </div>
          </div>
        </div>
      </div>

      {weak.length > 0 && (
        <p className="mt-[4px] mb-0 text-small text-ink-3">
          {t("settings.general.preferredLanguages.weak", {
            languages: languageList(weak, ui),
          })}
        </p>
      )}
    </SettingContainer>
  );
};

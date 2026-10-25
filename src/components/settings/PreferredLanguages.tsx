import React, { useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { SettingContainer } from "../ui/SettingContainer";
import {
  LanguageChip,
  LanguageOption,
  languageSearchKeys,
} from "../shared/LanguagePicker";
import { suggestedLanguages } from "../onboarding/data/languages";
import { isWeakLanguage } from "../onboarding/data/models";
import { useSettings } from "../../hooks/useSettings";
import {
  languageList,
  languageName,
  searchLanguages,
} from "../../lib/constants/languages";

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
  const [query, setQuery] = useState("");
  const [focused, setFocused] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const gridRef = useRef<HTMLDivElement>(null);

  const ui = i18n.language;
  const nameOf = (code: string) => languageName(code, ui);

  // Nothing picked yet (onboarded before this setting): suggest their language.
  const suggestion = languages.length ? null : suggestedLanguages()[0];
  const matches = useMemo(() => {
    const found = searchLanguages(query, ui);
    if (!suggestion || query) return found;
    return [
      ...found.filter((l) => l.code === suggestion),
      ...found.filter((l) => l.code !== suggestion),
    ];
  }, [query, ui, suggestion]);
  const available = matches.filter((l) => !languages.includes(l.code));
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

  const toggle = (code: string) => {
    const next = languages.includes(code)
      ? languages.filter((c) => c !== code)
      : [...languages, code];
    // At least one language, once there is one.
    if (!next.length) return;
    updateSetting("spoken_languages", next);
    setQuery("");
    gridRef.current?.scrollTo({ top: 0 });
    inputRef.current?.focus({ preventScroll: true });
  };

  const onKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
    const handled = languageSearchKeys(e, {
      query,
      first: available[0]?.code,
      last: languages.length > 1 ? languages[languages.length - 1] : undefined,
      toggle,
      clear: () => setQuery(""),
    });
    if (!handled && e.key === "Escape") inputRef.current?.blur();
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
          setQuery("");
        }}
        className={`mt-[2px] overflow-hidden rounded-card border-[1.5px] bg-background transition-colors duration-200 ${
          focused ? "border-ink-3" : "border-line-strong"
        }`}
      >
        {/* Clicks around the chips go to the search (a <label> would hand
            them to the first chip, which removes it). */}
        <div
          onClick={() => inputRef.current?.focus({ preventScroll: true })}
          className="flex min-h-[46px] cursor-text flex-wrap items-center gap-[4px] px-[8px] py-[7px] text-start"
        >
          {languages.map((code) => (
            <LanguageChip
              key={code}
              code={code}
              removeLabel={t("settings.general.preferredLanguages.remove", {
                language: nameOf(code),
              })}
              onRemove={() => toggle(code)}
              locked={languages.length === 1}
            />
          ))}
          <input
            ref={inputRef}
            type="text"
            autoComplete="off"
            spellCheck={false}
            aria-label={t("settings.general.preferredLanguages.searchLabel")}
            aria-expanded={open}
            placeholder={
              languages.length
                ? t("settings.general.preferredLanguages.addAnother")
                : t("settings.general.preferredLanguages.placeholder")
            }
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={onKeyDown}
            className="h-[30px] min-w-[140px] flex-1 border-0 bg-transparent px-[8px] text-small text-text outline-none placeholder:text-ink-3"
          />
        </div>

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
              aria-label={t("settings.general.preferredLanguages.searchLabel")}
              className={`grid max-h-[214px] grid-cols-[repeat(auto-fill,minmax(150px,1fr))] content-start gap-[2px] overflow-y-auto border-t border-line p-[6px] ${
                overflows
                  ? "pb-[14px] [mask-image:linear-gradient(to_bottom,black_calc(100%-36px),transparent_calc(100%-4px))]"
                  : ""
              }`}
            >
              {available.map((l, i) => (
                <LanguageOption
                  key={l.code}
                  language={l}
                  name={nameOf(l.code)}
                  highlighted={(!!query || !!suggestion) && i === 0}
                  onPick={() => toggle(l.code)}
                />
              ))}
              {!available.length && (
                <div className="col-span-full p-[20px] text-center text-small text-ink-3">
                  {matches.length
                    ? t("settings.general.preferredLanguages.alreadyAdded", {
                        count: matches.length,
                        languages: languageList(
                          matches.map((l) => l.code),
                          ui,
                        ),
                      })
                    : t("settings.general.preferredLanguages.noMatch", {
                        query: query.trim(),
                      })}
                </div>
              )}
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

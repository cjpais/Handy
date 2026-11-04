import React, { useEffect, useLayoutEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { SettingContainer } from "../ui/SettingContainer";
import { LanguageChip, useLanguagePicker } from "../shared/LanguagePicker";
import { suggestedLanguages } from "@/lib/utils/languageSuggestions";
import { isWeakLanguage } from "@/lib/models/catalog";
import { useSettings } from "../../hooks/useSettings";
import { isComposingKeyEvent } from "@/lib/utils/keyboard";
import { cjkSubsetFont } from "@/lib/utils/cjkFont";
import { languageList } from "../../lib/constants/languages";

interface PreferredLanguagesProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

type Picker = ReturnType<typeof useLanguagePicker>;

const PlusIcon = () => (
  <svg
    width="12"
    height="12"
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    strokeWidth={2.2}
    strokeLinecap="round"
    aria-hidden="true"
  >
    <path d="M12 5v14M5 12h14" />
  </svg>
);

const SearchIcon = () => (
  <svg
    width="14"
    height="14"
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    strokeWidth={2}
    strokeLinecap="round"
    aria-hidden="true"
  >
    <circle cx="11" cy="11" r="6.5" />
    <path d="M16 16l4 4" />
  </svg>
);

const CheckIcon = () => (
  <svg
    width="16"
    height="16"
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    strokeWidth={2.2}
    strokeLinecap="round"
    strokeLinejoin="round"
    aria-hidden="true"
  >
    <path d="M5 12.5l4.5 4.5L19 7.5" />
  </svg>
);

// Matches the row's gap-1.
const CHIP_GAP = 4;

/**
 * How many of `measure`'s chips fit in `box`, leaving room for "+N" (the
 * last child of `measure`) when some don't.
 */
const useChipsThatFit = (
  box: React.RefObject<HTMLDivElement | null>,
  measure: React.RefObject<HTMLDivElement | null>,
  languages: string[],
) => {
  const [count, setCount] = useState(languages.length);
  const key = languages.join();
  useLayoutEffect(() => {
    const b = box.current;
    const m = measure.current;
    if (!b || !m) return;
    const fit = () => {
      const kids = [...m.children] as HTMLElement[];
      const more = kids.pop()?.offsetWidth ?? 0;
      const room = b.clientWidth;
      let used = 0;
      let n = 0;
      for (const [i, kid] of kids.entries()) {
        const left = kids.length - i - 1;
        if (used + kid.offsetWidth + (left ? CHIP_GAP + more : 0) > room) break;
        used += kid.offsetWidth + CHIP_GAP;
        n++;
      }
      setCount(n);
    };
    fit();
    // CJK endonyms can arrive in a web font after the first measure.
    document.fonts?.ready.then(fit);
    const observer = new ResizeObserver(fit);
    observer.observe(b);
    return () => observer.disconnect();
  }, [box, measure, key]);
  return count;
};

/** The menu under +: a search over every language, the picked ones first and ticked. */
const LanguageMenu: React.FC<{
  picker: Picker;
  /** The picked languages when the menu opened: they lead, and stay put as they're toggled. */
  pinned: string[];
  /** Highlight the first addable even with nothing typed (it's the suggestion). */
  highlightFirst: boolean;
}> = ({ picker, pinned, highlightFirst }) => {
  const { t } = useTranslation();
  const { inputRef, query, languages, matches } = picker;
  // Picked ones in the chips' order, then the rest in search order.
  const rows = [
    ...pinned.flatMap((code) => matches.filter((l) => l.code === code)),
    ...matches.filter((l) => !pinned.includes(l.code)),
  ];
  const firstAddable = rows.find((l) => !languages.includes(l.code))?.code;

  return (
    <div
      // Keep focus in the search: WebKit doesn't focus a pressed button, so
      // the blur would close the menu before the click lands.
      onMouseDown={(e) => {
        if (e.target !== inputRef.current) e.preventDefault();
      }}
      className="absolute end-0 top-full z-50 mt-1 w-[248px] overflow-hidden rounded-lg border border-line bg-background text-start shadow-lg"
    >
      <label className="flex items-center gap-2 border-b border-line px-3 text-ink-3">
        <SearchIcon />
        <input
          ref={inputRef}
          type="text"
          autoComplete="off"
          spellCheck={false}
          aria-label={t("languagePicker.search")}
          placeholder={t("languagePicker.search")}
          value={query}
          onChange={(e) => picker.setQuery(e.target.value)}
          onKeyDown={(e) => {
            if (isComposingKeyEvent(e.nativeEvent)) return;
            // Escape with a query clears it; without one, the menu closes.
            if (picker.onKeyDown(e)) e.stopPropagation();
          }}
          className="h-9 w-full min-w-0 bg-transparent text-sm text-text outline-none placeholder:text-ink-3"
        />
      </label>
      <div className="max-h-56 overflow-y-auto p-1">
        {rows.map((l) => {
          const name = picker.nameOf(l.code);
          const picked = languages.includes(l.code);
          const locked = picked && !picker.removable;
          const highlighted =
            (!!query || highlightFirst) && l.code === firstAddable;
          return (
            <button
              key={l.code}
              type="button"
              aria-pressed={picked}
              disabled={locked}
              onClick={() => picker.toggle(l.code)}
              // Picked rows are only ticked: unlike Dropdown's one choice,
              // several fills would stack into stripes.
              className={`flex w-full cursor-pointer items-center gap-2 rounded-md px-2 py-1.5 text-start text-sm transition-colors disabled:cursor-default ${
                highlighted
                  ? "bg-wash"
                  : "hover:bg-wash disabled:hover:bg-transparent"
              }`}
            >
              <span
                lang={l.code}
                className={`shrink-0 ${picked ? "font-semibold" : ""} ${cjkSubsetFont(l.native) ?? ""}`}
              >
                {l.native}
              </span>
              {l.native !== name && (
                <span className="truncate text-xs text-ink-3">{name}</span>
              )}
              <span
                className={`ms-auto flex w-4 shrink-0 ${locked ? "text-ink-3" : "text-ink-2"}`}
              >
                {picked && <CheckIcon />}
              </span>
            </button>
          );
        })}
        {!rows.length && (
          <div className="px-2 py-3 text-center text-xs text-ink-3">
            {t("settings.general.preferredLanguages.noMatch", {
              query: query.trim(),
            })}
          </div>
        )}
      </div>
    </div>
  );
};

/**
 * Edits `spoken_languages`; leaves the model's own Language as is. One line
 * like its neighbours: as many chips as fit, "+N" for the rest, and + to
 * open the menu that adds and removes.
 */
export const PreferredLanguages: React.FC<PreferredLanguagesProps> = ({
  descriptionMode = "tooltip",
  grouped = false,
}) => {
  const { t, i18n } = useTranslation();
  const { getSetting, updateSetting } = useSettings();
  const languages = getSetting("spoken_languages") ?? [];
  const ui = i18n.language;

  // Nothing picked yet (onboarded before this setting): suggest their language.
  const suggestion = languages.length ? null : suggestedLanguages()[0];
  const picker = useLanguagePicker({
    languages,
    onChange: (next) => updateSetting("spoken_languages", next),
    // At least one language, once there is one.
    keepOne: true,
    suggestion,
  });

  const [open, setOpen] = useState(false);
  const [pinned, setPinned] = useState<string[]>([]);
  const rootRef = useRef<HTMLDivElement>(null);
  const addRef = useRef<HTMLButtonElement>(null);
  const boxRef = useRef<HTMLDivElement>(null);
  const measureRef = useRef<HTMLDivElement>(null);

  const fit = useChipsThatFit(boxRef, measureRef, languages);
  const shown = languages.slice(0, fit);
  const hidden = languages.slice(fit);

  const openMenu = () => {
    setPinned(languages);
    setOpen(true);
  };
  const closeMenu = () => {
    setOpen(false);
    picker.setQuery("");
  };
  const toggleMenu = () => (open ? closeMenu() : openMenu());

  useEffect(() => {
    if (open) picker.inputRef.current?.focus({ preventScroll: true });
  }, [open, picker.inputRef]);

  const chip = (code: string) => (
    <LanguageChip
      key={code}
      variant="soft"
      code={code}
      removeLabel={t("languagePicker.remove", {
        language: picker.nameOf(code),
      })}
      onRemove={() => {
        picker.toggle(code);
        // The chip goes, and with the menu shut there's no search to take
        // focus: give it to +.
        if (!open) addRef.current?.focus({ preventScroll: true });
      }}
      locked={!picker.removable}
    />
  );
  const moreLabel = hidden.length ? languageList(hidden, ui) : undefined;
  const moreClasses =
    "inline-flex h-[26px] shrink-0 items-center rounded-md px-2 text-xs font-medium tabular-nums";

  const weak = languages.filter(isWeakLanguage);

  return (
    <div>
      <SettingContainer
        title={t("settings.general.preferredLanguages.title")}
        description={t("settings.general.preferredLanguages.description")}
        descriptionMode={descriptionMode}
        grouped={grouped}
        grow
      >
        <div
          ref={rootRef}
          onBlur={(e) => {
            if (rootRef.current?.contains(e.relatedTarget as Node)) return;
            closeMenu();
          }}
          onKeyDown={(e) => {
            if (e.key !== "Escape" || !open) return;
            e.preventDefault();
            e.stopPropagation();
            closeMenu();
            addRef.current?.focus();
          }}
          className="relative flex min-w-0 flex-1 items-center gap-1"
        >
          <div
            ref={boxRef}
            className="relative flex min-w-0 flex-1 items-center justify-end gap-1 overflow-hidden"
          >
            {/* Every chip, unseen, to measure how many fit. */}
            <div
              ref={measureRef}
              aria-hidden="true"
              className="invisible absolute start-0 top-0 flex gap-1"
            >
              {languages.map(chip)}
              <span className={moreClasses}>{`+${languages.length}`}</span>
            </div>
            {shown.map(chip)}
            {hidden.length > 0 && (
              <button
                type="button"
                title={moreLabel}
                aria-label={moreLabel}
                aria-expanded={open}
                onMouseDown={(e) => e.preventDefault()}
                onClick={toggleMenu}
                className={`${moreClasses} cursor-pointer transition-colors hover:bg-line hover:text-text ${
                  open ? "bg-line text-text" : "bg-wash text-ink-2"
                }`}
              >
                {`+${hidden.length}`}
              </button>
            )}
          </div>
          <button
            ref={addRef}
            type="button"
            aria-label={
              languages.length
                ? t("languagePicker.addAnother")
                : t("languagePicker.search")
            }
            aria-expanded={open}
            onMouseDown={(e) => e.preventDefault()}
            onClick={toggleMenu}
            className={`inline-flex h-[26px] shrink-0 cursor-pointer items-center justify-center gap-1 whitespace-nowrap rounded-md text-xs font-medium transition-colors hover:bg-wash hover:text-text ${
              languages.length ? "w-[26px]" : "px-2"
            } ${open ? "bg-wash text-text" : "text-ink-2"}`}
          >
            <PlusIcon />
            {/* With nothing picked, + alone would say too little. */}
            {!languages.length && t("languagePicker.search")}
          </button>
          {open && (
            <LanguageMenu
              picker={picker}
              pinned={pinned}
              highlightFirst={!!suggestion}
            />
          )}
        </div>
      </SettingContainer>

      {weak.length > 0 && (
        <p className="-mt-1 mb-0 px-4 pb-2 text-end text-xs text-ink-3">
          {t("settings.general.preferredLanguages.weak", {
            languages: languageList(weak, ui),
          })}
        </p>
      )}
    </div>
  );
};

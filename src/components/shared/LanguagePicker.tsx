import React, { useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  getLanguage,
  languageList,
  languageName,
  searchLanguages,
  type Language,
} from "@/lib/constants/languages";
import { cjkSubsetFont } from "@/lib/utils/cjkFont";

/** Spoken-language picker pieces shared by onboarding's Welcome and the Preferred Languages setting. */

const RemoveIcon = () => (
  <svg
    width="10"
    height="10"
    viewBox="0 0 10 10"
    fill="none"
    stroke="currentColor"
    strokeWidth="1.6"
    strokeLinecap="round"
    aria-hidden="true"
  >
    <path d="M2 2l6 6M8 2L2 8" />
  </svg>
);

interface LanguageChipProps {
  code: string;
  /** What the button does, for screen readers ("Remove English"). */
  removeLabel: string;
  onRemove: () => void;
  /** The last language can't go: the chip stays, without its ×. */
  locked?: boolean;
}

/** A picked language by its endonym, with a remove button that keeps focus in the search. */
export const LanguageChip: React.FC<LanguageChipProps> = ({
  code,
  removeLabel,
  onRemove,
  locked = false,
}) => {
  const { native } = getLanguage(code);
  const name = (
    <bdi lang={code} className={cjkSubsetFont(native)}>
      {native}
    </bdi>
  );
  if (locked)
    return (
      <span className="inline-flex h-[28px] items-center rounded-full border-[1.5px] border-line-strong bg-background px-[12px] text-small font-semibold text-text">
        {name}
      </span>
    );
  return (
    <button
      type="button"
      aria-label={removeLabel}
      onMouseDown={(e) => e.preventDefault()}
      onClick={(e) => {
        e.preventDefault();
        onRemove();
      }}
      className="group inline-flex h-[28px] cursor-pointer items-center gap-[6px] rounded-full border-[1.5px] border-line-strong bg-background ps-[12px] pe-[6px] text-small font-semibold text-text hover:border-ink-3"
    >
      {name}
      <span className="grid size-[18px] place-items-center rounded-full leading-none text-ink-2 group-hover:bg-line">
        <RemoveIcon />
      </span>
    </button>
  );
};

interface LanguageOptionProps {
  language: Language;
  /** Its name in the UI language. */
  name: string;
  /** What Enter in the search adds. */
  highlighted?: boolean;
  onPick: () => void;
}

/** A language to add: endonym over its UI-language name (blank when equal, so cells align). */
export const LanguageOption: React.FC<LanguageOptionProps> = ({
  language,
  name,
  highlighted = false,
  onPick,
}) => (
  <button
    type="button"
    onClick={onPick}
    className={`flex h-[46px] cursor-pointer flex-col justify-center rounded-[9px] px-[12px] text-start leading-[1.25] hover:bg-wash ${
      highlighted
        ? "border-[1.5px] border-line-strong bg-wash"
        : "border border-transparent"
    }`}
  >
    <b
      lang={language.code}
      className={`font-semibold ${cjkSubsetFont(language.native) ?? ""}`}
    >
      {language.native}
    </b>
    <small className="text-label text-ink-3">
      {language.native !== name ? name : "\u00a0"}
    </small>
  </button>
);

/** Search keys: Enter adds, Backspace on empty removes the last, Escape clears. Returns true if handled. */
export const languageSearchKeys = (
  e: React.KeyboardEvent<HTMLInputElement>,
  {
    query,
    first,
    last,
    toggle,
    clear,
  }: {
    query: string;
    /** The language Enter adds. */
    first: string | undefined;
    /** The language Backspace removes (none when it can't). */
    last: string | undefined;
    toggle: (code: string) => void;
    clear: () => void;
  },
): boolean => {
  if (e.nativeEvent.isComposing) return false;
  if (e.key === "Enter" && query) {
    e.preventDefault();
    if (first) toggle(first);
    return true;
  }
  if (e.key === "Backspace" && !query && last) {
    e.preventDefault();
    toggle(last);
    return true;
  }
  if (e.key === "Escape" && query) {
    clear();
    return true;
  }
  return false;
};

/**
 * The picker's search: what matches, what's left to add, and toggling. With
 * `keepOne` the last language can't be removed; a `suggestion` leads the list
 * until something is typed.
 */
export const useLanguagePicker = ({
  languages,
  onChange,
  keepOne = false,
  suggestion = null,
}: {
  languages: string[];
  onChange: (next: string[]) => void;
  keepOne?: boolean;
  suggestion?: string | null;
}) => {
  const { i18n } = useTranslation();
  const ui = i18n.language;
  const [query, setQuery] = useState("");
  const inputRef = useRef<HTMLInputElement>(null);

  const matches = useMemo(() => {
    const found = searchLanguages(query, ui);
    if (!suggestion || query) return found;
    return [
      ...found.filter((l) => l.code === suggestion),
      ...found.filter((l) => l.code !== suggestion),
    ];
  }, [query, ui, suggestion]);
  const available = matches.filter((l) => !languages.includes(l.code));
  const removable = !keepOne || languages.length > 1;

  const toggle = (code: string) => {
    const next = languages.includes(code)
      ? languages.filter((c) => c !== code)
      : [...languages, code];
    if (!next.length && keepOne) return;
    onChange(next);
    setQuery("");
    inputRef.current?.focus({ preventScroll: true });
  };

  const onKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) =>
    languageSearchKeys(e, {
      query,
      first: available[0]?.code,
      last: removable ? languages[languages.length - 1] : undefined,
      toggle,
      clear: () => setQuery(""),
    });

  return {
    languages,
    query,
    setQuery,
    inputRef,
    matches,
    available,
    removable,
    toggle,
    onKeyDown,
    nameOf: (code: string) => languageName(code, ui),
  };
};

type Picker = ReturnType<typeof useLanguagePicker>;

/** The picked languages as chips, then the search. Clicks around the chips go to the search. */
export const LanguageSearchField: React.FC<{
  picker: Picker;
  className?: string;
  inputClassName?: string;
  onKeyDown?: (e: React.KeyboardEvent<HTMLInputElement>) => void;
  "aria-expanded"?: boolean;
}> = ({
  picker,
  className = "",
  inputClassName = "",
  onKeyDown = picker.onKeyDown,
  "aria-expanded": expanded,
}) => {
  const { t } = useTranslation();
  const { languages, inputRef } = picker;
  return (
    // Not a <label>: it would hand clicks to the first chip, which removes it.
    <div
      onClick={() => inputRef.current?.focus({ preventScroll: true })}
      className={`flex min-h-[46px] cursor-text flex-wrap items-center gap-[4px] px-[8px] py-[7px] text-start ${className}`}
    >
      {languages.map((code) => (
        <LanguageChip
          key={code}
          code={code}
          removeLabel={t("languagePicker.remove", {
            language: picker.nameOf(code),
          })}
          onRemove={() => picker.toggle(code)}
          locked={!picker.removable}
        />
      ))}
      <input
        ref={inputRef}
        type="text"
        autoComplete="off"
        spellCheck={false}
        aria-label={t("languagePicker.search")}
        aria-expanded={expanded}
        placeholder={
          languages.length
            ? t("languagePicker.addAnother")
            : t("languagePicker.search")
        }
        value={picker.query}
        onChange={(e) => picker.setQuery(e.target.value)}
        onKeyDown={onKeyDown}
        className={`h-[30px] flex-1 border-0 bg-transparent px-[8px] text-text outline-none placeholder:text-ink-3 ${inputClassName}`}
      />
    </div>
  );
};

/** The languages left to add, the first highlighted while it's what Enter adds; when none, why. */
export const LanguageOptions: React.FC<{
  picker: Picker;
  /** Highlight the first even with nothing typed (it's the suggestion). */
  highlightFirst?: boolean;
  /** Nothing matches the query. */
  noMatch: string;
  emptyClassName?: string;
}> = ({ picker, highlightFirst = false, noMatch, emptyClassName = "" }) => {
  const { t, i18n } = useTranslation();
  const { available, matches, query } = picker;
  return (
    <>
      {available.map((l, i) => (
        <LanguageOption
          key={l.code}
          language={l}
          name={picker.nameOf(l.code)}
          highlighted={(!!query || highlightFirst) && i === 0}
          onPick={() => picker.toggle(l.code)}
        />
      ))}
      {!available.length && (
        <div
          className={`col-span-full text-center text-small text-ink-3 ${emptyClassName}`}
        >
          {matches.length
            ? t("languagePicker.alreadyAdded", {
                count: matches.length,
                languages: languageList(
                  matches.map((l) => l.code),
                  i18n.language,
                ),
              })
            : noMatch}
        </div>
      )}
    </>
  );
};

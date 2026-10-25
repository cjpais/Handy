import React from "react";
import { getLanguage, type Language } from "@/lib/constants/languages";

/**
 * The parts of picking the languages someone speaks, shared by onboarding's
 * Welcome and the Preferred Languages setting: a chip for each picked
 * language, a cell for each one to pick, and the search's keys.
 */

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

/**
 * A language someone speaks, by its own name (the endonym, the same in every
 * UI language), with a × that removes it. Pressing it keeps focus in the
 * search beside it.
 */
export const LanguageChip: React.FC<LanguageChipProps> = ({
  code,
  removeLabel,
  onRemove,
  locked = false,
}) => {
  const name = <bdi lang={code}>{getLanguage(code).native}</bdi>;
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

/**
 * A language to add: its own name over its name in the UI language (left
 * blank when they're the same, so the cells line up).
 */
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
    <b lang={language.code} className="font-semibold">
      {language.native}
    </b>
    <small className="text-label text-ink-3">
      {language.native !== name ? name : "\u00a0"}
    </small>
  </button>
);

/**
 * The keys of a language search beside its chips: Enter adds the first match,
 * Backspace in an empty search removes the last language, Escape clears the
 * search. Returns whether it handled the key.
 */
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

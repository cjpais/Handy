import React, { useId, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { useSettings } from "../../../hooks/useSettings";
import { Button } from "../../ui/Button";
import { Input } from "../../ui/Input";
import {
  WORDS_MAX_LENGTH,
  filterWords,
  normalizeWord,
  validateWord,
} from "./dictionaryFile";

interface DictionaryCustomWordsProps {
  /** Current dictionary search query (manages/persists nothing itself). */
  filter: string;
}

/**
 * Custom Words section of the Dictionary page.
 *
 * Reuses Handy's existing `custom_words` setting and normalization rules, so
 * entries added here (or previously added in Advanced settings) feed the
 * existing fuzzy matcher in the transcription pipeline unchanged.
 */
export const DictionaryCustomWords: React.FC<DictionaryCustomWordsProps> = ({
  filter,
}) => {
  const { t } = useTranslation();
  const { getSetting, updateSetting, isUpdating } = useSettings();
  const [newWord, setNewWord] = useState("");
  const addInputId = useId();

  const customWords = getSetting("custom_words") || [];
  const updating = isUpdating("custom_words");
  const normalizedWord = normalizeWord(newWord);
  const visibleWords = useMemo(
    () => filterWords(customWords, filter),
    [customWords, filter],
  );

  const focusAddInput = () => {
    document.getElementById(addInputId)?.focus();
  };

  const handleAddWord = async () => {
    const error = validateWord(newWord, customWords);
    if (error === "duplicate") {
      toast.error(
        t("settings.dictionary.customWords.duplicate", {
          word: normalizedWord,
        }),
      );
      return;
    }
    if (error) return;
    await updateSetting("custom_words", [...customWords, normalizedWord]);
    setNewWord("");
    // Focused after the write resolves: the input is `disabled` while
    // `updating`, and a disabled element cannot take focus.
    focusAddInput();
  };

  const handleRemoveWord = async (wordToRemove: string) => {
    await updateSetting(
      "custom_words",
      customWords.filter((word) => word !== wordToRemove),
    );
    // The removed chip's button disappears with its element; keep keyboard
    // focus inside the section instead of dropping it on <body>. Focused after
    // the write resolves, because every control is `disabled` while `updating`.
    focusAddInput();
  };

  return (
    <div className="space-y-2">
      <div className="px-4 flex items-baseline justify-between gap-4">
        <div>
          <h2 className="text-xs font-medium text-mid-gray uppercase tracking-wide">
            {t("settings.dictionary.customWords.title")}
          </h2>
          <p className="text-xs text-mid-gray mt-1">
            {t("settings.dictionary.customWords.description")}
          </p>
        </div>
        <span className="shrink-0 text-xs text-mid-gray">
          {t("settings.dictionary.customWords.count", {
            count: visibleWords.length,
          })}
        </span>
      </div>
      <div className="bg-background border border-mid-gray/20 rounded-lg overflow-visible">
        <div className="divide-y divide-mid-gray/20">
          <div className="p-2 flex items-center gap-2">
            <Input
              id={addInputId}
              type="text"
              variant="compact"
              className="flex-1"
              value={newWord}
              onChange={(e) => setNewWord(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") {
                  e.preventDefault();
                  void handleAddWord();
                }
              }}
              placeholder={t("settings.dictionary.customWords.placeholder")}
              aria-label={t("settings.dictionary.customWords.placeholder")}
              disabled={updating}
            />
            <Button
              onClick={() => void handleAddWord()}
              disabled={
                !normalizedWord ||
                normalizedWord.length > WORDS_MAX_LENGTH ||
                updating
              }
              variant="primary"
              size="md"
            >
              {t("settings.dictionary.customWords.add")}
            </Button>
          </div>
          {customWords.length === 0 ? (
            <p className="px-4 py-3 text-sm text-mid-gray">
              {t("settings.dictionary.customWords.empty")}
            </p>
          ) : visibleWords.length === 0 ? (
            <p className="px-4 py-3 text-sm text-mid-gray">
              {t("settings.dictionary.search.noResults")}
            </p>
          ) : (
            <div className="px-4 p-2 flex flex-wrap gap-1">
              {visibleWords.map((word) => (
                <Button
                  key={word}
                  onClick={() => void handleRemoveWord(word)}
                  disabled={updating}
                  variant="secondary"
                  size="sm"
                  className="inline-flex items-center gap-1 cursor-pointer"
                  aria-label={t("settings.dictionary.customWords.remove", {
                    word,
                  })}
                >
                  <span>{word}</span>
                  <svg
                    className="w-3 h-3"
                    fill="none"
                    stroke="currentColor"
                    viewBox="0 0 24 24"
                    aria-hidden="true"
                  >
                    <path
                      strokeLinecap="round"
                      strokeLinejoin="round"
                      strokeWidth={2}
                      d="M6 18L18 6M6 6l12 12"
                    />
                  </svg>
                </Button>
              ))}
            </div>
          )}
        </div>
      </div>
    </div>
  );
};

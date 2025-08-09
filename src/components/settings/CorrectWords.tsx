import React, { useState } from "react";
import { useSettings } from "../../hooks/useSettings";
import { Input } from "../ui/Input";
import { Button } from "../ui/Button";
import { SettingContainer } from "../ui/SettingContainer";

interface CorrectWordsProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const CorrectWords: React.FC<CorrectWordsProps> = React.memo(({
  descriptionMode = "tooltip",
  grouped = false,
}) => {
  const { getSetting, updateSetting, isUpdating } = useSettings();
  const [newWord, setNewWord] = useState("");
  const correctWords = getSetting("correct_words") || [];

  const handleAddWord = () => {
    const trimmedWord = newWord.trim();
    const sanitizedWord = trimmedWord.replace(/[<>"'&]/g, '');
    if (sanitizedWord && !sanitizedWord.includes(' ') && sanitizedWord.length <= 50 && !correctWords.includes(sanitizedWord)) {
      updateSetting("correct_words", [...correctWords, sanitizedWord]);
      setNewWord("");
    }
  };

  const handleRemoveWord = (wordToRemove: string) => {
    updateSetting("correct_words", correctWords.filter((word) => word !== wordToRemove));
  };

  const handleKeyPress = (e: React.KeyboardEvent) => {
    if (e.key === "Enter") {
      e.preventDefault();
      handleAddWord();
    }
  };

  return (
    <>
      <SettingContainer
        title="Custom Words"
        description="Add words that are often misheard or misspelled during transcription. The system will automatically correct similar-sounding words to match your list."
        descriptionMode={descriptionMode}
        grouped={grouped}
      >
        <div className="flex items-center gap-2">
          <Input
            type="text"
            className="max-w-[128px]"
            value={newWord}
            onChange={(e) => setNewWord(e.target.value)}
            onKeyPress={handleKeyPress}
            placeholder="Add a word"
            variant="compact"
            disabled={isUpdating("correct_words")}
          />
          <Button
            onClick={handleAddWord}
            disabled={!newWord.trim() || newWord.includes(' ') || newWord.trim().length > 50 || isUpdating("correct_words")}
            variant="primary"
            size="md"
          >
            Add
          </Button>
        </div>
      </SettingContainer>
      {correctWords.length > 0 && (
        <div className={`px-4 p-2 ${grouped ? "" : "rounded-lg border border-mid-gray/20"} flex flex-wrap gap-1`}>
          {correctWords.map((word) => (
            <Button
              key={word}
              onClick={() => handleRemoveWord(word)}
              disabled={isUpdating("correct_words")}
              variant="secondary"
              size="sm"
              className="inline-flex items-center gap-1 cursor-pointer"
              aria-label={`Remove ${word}`}
            >
              <span>{word}</span>
              <svg className="w-3 h-3" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M6 18L18 6M6 6l12 12" />
              </svg>
            </Button>
          ))}
        </div>
      )}
    </>
  );
});
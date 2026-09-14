import React, { useState } from "react";
import { useTranslation } from "react-i18next";
import { Input } from "../../ui/Input";
import { DictionaryCustomWords } from "./DictionaryCustomWords";
import { DictionaryImportExport } from "./DictionaryImportExport";
import { DictionaryReplacements } from "./DictionaryReplacements";

/**
 * Dictionary settings page (fork feature).
 *
 * Two related but distinct systems:
 * - Custom Words: reuse the existing `custom_words` setting and its fuzzy
 *   matcher in the transcription pipeline.
 * - Replacements: deterministic final-output corrections
 *   (`dictionary_replacements`, applied in process_transcription_output).
 *
 * Search is local and case-insensitive; it filters both sections without
 * reordering or mutating anything persisted.
 */
export const DictionarySettings: React.FC = () => {
  const { t } = useTranslation();
  const [search, setSearch] = useState("");

  return (
    <div className="max-w-3xl w-full mx-auto space-y-6">
      <div className="flex flex-col gap-2 sm:flex-row sm:items-center">
        <Input
          type="text"
          className="flex-1"
          value={search}
          onChange={(e) => setSearch(e.target.value)}
          placeholder={t("settings.dictionary.search.placeholder")}
          aria-label={t("settings.dictionary.search.label")}
        />
        <DictionaryImportExport />
      </div>
      <DictionaryCustomWords filter={search} />
      <DictionaryReplacements filter={search} />
    </div>
  );
};

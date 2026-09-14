import React, { useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { open, save } from "@tauri-apps/plugin-dialog";
import { readTextFile, writeTextFile } from "@tauri-apps/plugin-fs";
import { useSettings } from "../../../hooks/useSettings";
import { Button } from "../../ui/Button";
import { Dialog } from "../../ui/Dialog";
import {
  DictionaryParseError,
  buildDictionaryJson,
  buildWordsTxt,
  mergeReplacements,
  mergeWords,
  parseDictionaryFile,
  serializeReplacementsCsv,
  type DictionaryRule,
} from "./dictionaryFile";

type ExportFormat = "json" | "txt" | "csv";

const EXPORT_FILES: Record<
  ExportFormat,
  { fileName: string; extension: string }
> = {
  json: { fileName: "handy-dictionary.json", extension: "json" },
  txt: { fileName: "handy-custom-words.txt", extension: "txt" },
  csv: { fileName: "handy-replacements.csv", extension: "csv" },
};

/**
 * Import / Export controls for the Dictionary page.
 *
 * Files are only read or written in response to an explicit user action, via
 * the standard Tauri dialog; parsing happens before anything is persisted so a
 * bad file can never modify settings partially. Imports merge with existing
 * entries and never replace them.
 */
export const DictionaryImportExport: React.FC = () => {
  const { t } = useTranslation();
  const { getSetting, updateSetting } = useSettings();
  const [isExportOpen, setIsExportOpen] = useState(false);
  const [isImporting, setIsImporting] = useState(false);
  const [isExporting, setIsExporting] = useState(false);

  const handleImport = async () => {
    if (isImporting) return;
    let selected: string | null = null;
    try {
      const result = await open({
        multiple: false,
        directory: false,
        filters: [
          {
            name: t("settings.dictionary.title"),
            extensions: ["json", "txt", "csv"],
          },
        ],
      });
      selected = typeof result === "string" ? result : null;
    } catch (error) {
      console.error("Dictionary import dialog failed:", error);
      return;
    }
    if (!selected) return;

    setIsImporting(true);
    try {
      let content: string;
      try {
        content = await readTextFile(selected);
      } catch (error) {
        console.error("Dictionary import read failed:", error);
        toast.error(t("settings.dictionary.errors.readFailed"));
        return;
      }

      // Parse and validate the whole file BEFORE touching persisted settings,
      // so a malformed import is never applied partially.
      const parsed = parseDictionaryFile(selected, content);
      const currentWords = getSetting("custom_words") || [];
      const currentRules = (getSetting("dictionary_replacements") ||
        []) as DictionaryRule[];
      const wordsResult = mergeWords(currentWords, parsed.words);
      const rulesResult = mergeReplacements(currentRules, parsed.replacements);

      if (wordsResult.added > 0) {
        await updateSetting("custom_words", wordsResult.words);
      }
      if (rulesResult.added > 0) {
        await updateSetting("dictionary_replacements", rulesResult.rules);
      }

      // Summary only when something was actually applied; everything that was
      // not applied is reported by kind below, so counts are never conflated
      // (words and rules use the same buckets: added / duplicates / invalid).
      if (wordsResult.added > 0 || rulesResult.added > 0) {
        toast.success(
          t("settings.dictionary.importResults.summary", {
            words: wordsResult.added,
            replacements: rulesResult.added,
          }),
        );
      }
      if (wordsResult.duplicates > 0) {
        toast.info(
          t("settings.dictionary.importResults.skipped", {
            count: wordsResult.duplicates,
          }),
        );
      }
      if (rulesResult.conflicts > 0) {
        toast.info(
          t("settings.dictionary.importResults.conflicts", {
            count: rulesResult.conflicts,
          }),
        );
      }
      // Blank and over-long entries are not duplicates: word-side blanks /
      // over-length words plus rule-side blank fields.
      const invalidEntries = wordsResult.invalid + rulesResult.skipped;
      if (invalidEntries > 0) {
        toast.info(
          t("settings.dictionary.importResults.invalid", {
            count: invalidEntries,
            // Placeholder until the key lands in the locale files.
            defaultValue: "{{count}} blank or over-long entries skipped",
          }),
        );
      }
      if (parsed.malformedRows > 0) {
        toast.info(
          t("settings.dictionary.importResults.malformedRows", {
            count: parsed.malformedRows,
          }),
        );
      }
    } catch (error) {
      if (error instanceof DictionaryParseError) {
        // The raw parser detail (JSON.parse text, version numbers) is never
        // shown to the user: it goes to the console, the toast is translated.
        console.error("Dictionary import failed:", error);
        if (error.code === "unsupportedVersion") {
          toast.error(
            t("settings.dictionary.errors.unsupportedVersion", {
              version: error.detail ?? "",
            }),
          );
        } else {
          // invalidJson / invalidFormat / malformedCsv all mean the file is not
          // a readable Handy dictionary.
          toast.error(t("settings.dictionary.errors.invalidFormat"));
        }
      } else {
        console.error("Dictionary import failed:", error);
        toast.error(t("settings.dictionary.errors.readFailed"));
      }
    } finally {
      setIsImporting(false);
    }
  };

  const handleExport = async (format: ExportFormat) => {
    setIsExportOpen(false);
    if (isExporting) return;
    const words = getSetting("custom_words") || [];
    const rules = (getSetting("dictionary_replacements") ||
      []) as DictionaryRule[];
    const { fileName, extension } = EXPORT_FILES[format];
    let content: string;
    if (format === "json") {
      content = buildDictionaryJson(words, rules);
    } else if (format === "txt") {
      content = buildWordsTxt(words);
    } else {
      content = serializeReplacementsCsv(rules);
    }

    setIsExporting(true);
    try {
      const path = await save({
        defaultPath: fileName,
        filters: [{ name: extension.toUpperCase(), extensions: [extension] }],
      });
      if (!path) return;
      await writeTextFile(path, content);
      toast.success(t("settings.dictionary.exportSuccess"));
    } catch (error) {
      console.error("Dictionary export failed:", error);
      toast.error(
        t("settings.dictionary.errors.writeFailed", {
          message: error instanceof Error ? error.message : String(error),
        }),
      );
    } finally {
      setIsExporting(false);
    }
  };

  return (
    <div className="flex items-center gap-2">
      <Button
        variant="secondary"
        size="md"
        onClick={handleImport}
        disabled={isImporting}
      >
        {t("settings.dictionary.import")}
      </Button>
      <Button
        variant="secondary"
        size="md"
        onClick={() => setIsExportOpen(true)}
        disabled={isExporting}
      >
        {t("settings.dictionary.export")}
      </Button>
      <Dialog
        open={isExportOpen}
        onOpenChange={setIsExportOpen}
        title={t("settings.dictionary.export")}
        closeLabel={t("common.close")}
      >
        <div className="flex flex-col gap-2">
          <Button
            variant="secondary"
            size="md"
            className="w-full"
            onClick={() => handleExport("json")}
          >
            {t("settings.dictionary.exportOptions.json")}
          </Button>
          <Button
            variant="secondary"
            size="md"
            className="w-full"
            onClick={() => handleExport("txt")}
          >
            {t("settings.dictionary.exportOptions.txt")}
          </Button>
          <Button
            variant="secondary"
            size="md"
            className="w-full"
            onClick={() => handleExport("csv")}
          >
            {t("settings.dictionary.exportOptions.csv")}
          </Button>
        </div>
      </Dialog>
    </div>
  );
};

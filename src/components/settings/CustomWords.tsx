import React, { useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { commands } from "../../bindings";
import { useSettings } from "../../hooks/useSettings";
import { Input } from "../ui/Input";
import { Button } from "../ui/Button";
import { SettingContainer } from "../ui/SettingContainer";

const WORD_LISTS_BASE = import.meta.env.DEV
  ? "http://localhost:1420/word-lists/"
  : "https://raw.githubusercontent.com/cjpais/Handy/main/word-lists/";

interface WordListMeta {
  id: string;
  name: string;
  description: string;
  file: string;
  wordCount: number;
}

interface CustomWordsProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

function sanitizeWord(raw: string): string {
  return raw.replace(/[<>"'&]/g, "").trim();
}

function parseWords(text: string): string[] {
  return text
    .split(/[,\n]/)
    .map(sanitizeWord)
    .filter((w) => w.length > 0 && w.length <= 50);
}

export const CustomWords: React.FC<CustomWordsProps> = React.memo(
  ({ descriptionMode = "tooltip", grouped = false }) => {
    const { t } = useTranslation();
    const { getSetting, updateSetting, isUpdating } = useSettings();
    const [newWord, setNewWord] = useState("");
    const customWords = getSetting("custom_words") || [];

    // Import section state
    const [importOpen, setImportOpen] = useState(false);
    const [importTab, setImportTab] = useState<"paste" | "url" | "ai">("paste");
    const [pasteText, setPasteText] = useState("");
    const [urlText, setUrlText] = useState("");
    const [urlFetching, setUrlFetching] = useState(false);
    const [urlWords, setUrlWords] = useState<string[] | null>(null);


    // Browse section state
    const [browseOpen, setBrowseOpen] = useState(false);
    const [lists, setLists] = useState<WordListMeta[] | null>(null);
    const [listsLoading, setListsLoading] = useState(false);
    const [expandedList, setExpandedList] = useState<string | null>(null);
    const [listWords, setListWords] = useState<Record<string, string[]>>({});
    const [listLoading, setListLoading] = useState<Record<string, boolean>>({});

    const handleAddWord = () => {
      const sanitizedWord = sanitizeWord(newWord);
      if (
        sanitizedWord &&
        !sanitizedWord.includes(" ") &&
        sanitizedWord.length <= 50
      ) {
        if (customWords.includes(sanitizedWord)) {
          toast.error(
            t("settings.advanced.customWords.duplicate", {
              word: sanitizedWord,
            }),
          );
          return;
        }
        updateSetting("custom_words", [...customWords, sanitizedWord]);
        setNewWord("");
      }
    };

    const handleRemoveWord = (wordToRemove: string) => {
      updateSetting(
        "custom_words",
        customWords.filter((word) => word !== wordToRemove),
      );
    };

    const handleKeyPress = (e: React.KeyboardEvent) => {
      if (e.key === "Enter") {
        e.preventDefault();
        handleAddWord();
      }
    };

    // Bulk add helper — returns [added, skipped]
    const bulkAdd = (words: string[]): [number, number] => {
      const unique = words.filter((w) => !customWords.includes(w));
      const skipped = words.length - unique.length;
      if (unique.length > 0) {
        updateSetting("custom_words", [...customWords, ...unique]);
      }
      return [unique.length, skipped];
    };

    const parsedPasteWords = parseWords(pasteText);

    const handleAddPaste = () => {
      if (parsedPasteWords.length === 0) {
        toast.error(t("settings.advanced.customWords.importEmpty"));
        return;
      }
      const [added, skipped] = bulkAdd(parsedPasteWords);
      toast.success(
        t("settings.advanced.customWords.importResult", { added, skipped }),
      );
      setPasteText("");
      setImportOpen(false);
    };

    const handleFetchUrl = async () => {
      const url = urlText.trim();
      if (!url) return;
      setUrlFetching(true);
      setUrlWords(null);
      const result = await commands.fetchWordList(url);
      setUrlFetching(false);
      if (result.status === "error") {
        toast.error(
          t("settings.advanced.customWords.importError", {
            error: result.error,
          }),
        );
        return;
      }
      const words = result.data
        .map(sanitizeWord)
        .filter((w) => w.length > 0 && w.length <= 50);
      setUrlWords(words);
    };

    const AI_PROMPT =
      "Based on our conversation and the technical context you have, generate a plain text list of words, proper nouns, technical terms, or acronyms that a speech-to-text model might mishear or misspell. Return one word or short phrase per line, with no bullet points, no numbering, and no explanations.";

    const handleCopyAiPrompt = () => {
      navigator.clipboard.writeText(AI_PROMPT);
      toast.success(t("settings.advanced.customWords.promptCopied"));
    };

    const handleAddUrl = () => {
      if (!urlWords || urlWords.length === 0) {
        toast.error(t("settings.advanced.customWords.importEmpty"));
        return;
      }
      const [added, skipped] = bulkAdd(urlWords);
      toast.success(
        t("settings.advanced.customWords.importResult", { added, skipped }),
      );
      setUrlWords(null);
      setUrlText("");
      setImportOpen(false);
    };

    const handleBrowseOpen = async () => {
      const next = !browseOpen;
      setBrowseOpen(next);
      if (next && lists === null) {
        setListsLoading(true);
        const result = await commands.fetchWordList(
          WORD_LISTS_BASE + "index.json",
        );
        setListsLoading(false);
        if (result.status === "error") {
          toast.error(
            t("settings.advanced.customWords.importError", {
              error: result.error,
            }),
          );
          return;
        }
        try {
          const parsed: WordListMeta[] = JSON.parse(result.data.join("\n"));
          setLists(parsed);
        } catch {
          toast.error(
            t("settings.advanced.customWords.importError", {
              error: "Invalid manifest",
            }),
          );
        }
      }
    };

    const handleExpandList = async (meta: WordListMeta) => {
      const id = meta.id;
      if (expandedList === id) {
        setExpandedList(null);
        return;
      }
      setExpandedList(id);
      if (!listWords[id]) {
        setListLoading((prev) => ({ ...prev, [id]: true }));
        const result = await commands.fetchWordList(
          WORD_LISTS_BASE + meta.file,
        );
        setListLoading((prev) => ({ ...prev, [id]: false }));
        if (result.status === "error") {
          toast.error(
            t("settings.advanced.customWords.importError", {
              error: result.error,
            }),
          );
          return;
        }
        const words = result.data
          .map(sanitizeWord)
          .filter((w) => w.length > 0 && w.length <= 50);
        setListWords((prev) => ({ ...prev, [id]: words }));
      }
    };

    const handleAddListWords = (id: string) => {
      const words = listWords[id];
      if (!words || words.length === 0) return;
      const [added, skipped] = bulkAdd(words);
      toast.success(
        t("settings.advanced.customWords.importResult", { added, skipped }),
      );
    };

    return (
      <>
        <SettingContainer
          title={t("settings.advanced.customWords.title")}
          description={t("settings.advanced.customWords.description")}
          descriptionMode={descriptionMode}
          grouped={grouped}
        >
          <div className="flex flex-col gap-3 w-full">
            {/* Single word input */}
            <div className="flex items-center gap-2">
              <Input
                type="text"
                className="max-w-40"
                value={newWord}
                onChange={(e) => setNewWord(e.target.value)}
                onKeyDown={handleKeyPress}
                placeholder={t("settings.advanced.customWords.placeholder")}
                variant="compact"
                disabled={isUpdating("custom_words")}
              />
              <Button
                onClick={handleAddWord}
                disabled={
                  !newWord.trim() ||
                  newWord.includes(" ") ||
                  newWord.trim().length > 50 ||
                  isUpdating("custom_words")
                }
                variant="primary"
                size="md"
              >
                {t("settings.advanced.customWords.add")}
              </Button>
            </div>

            {/* Import section */}
            <div className="border border-mid-gray/20 rounded-lg overflow-hidden">
              <button
                className="w-full flex items-center justify-between px-3 py-2 text-sm font-medium hover:bg-mid-gray/10 transition-colors cursor-pointer"
                onClick={() => setImportOpen((v) => !v)}
              >
                <span>{t("settings.advanced.customWords.import")}</span>
                <svg
                  className={`w-4 h-4 transition-transform ${importOpen ? "rotate-180" : ""}`}
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

              {importOpen && (
                <div className="border-t border-mid-gray/20 p-3 flex flex-col gap-3">
                  {/* Tabs */}
                  <div className="flex gap-1">
                    <button
                      className={`px-3 py-1 text-xs rounded-md cursor-pointer transition-colors ${importTab === "paste" ? "bg-background-ui text-white" : "hover:bg-mid-gray/10"}`}
                      onClick={() => setImportTab("paste")}
                    >
                      {t("settings.advanced.customWords.importPaste")}
                    </button>
                    <button
                      className={`px-3 py-1 text-xs rounded-md cursor-pointer transition-colors ${importTab === "url" ? "bg-background-ui text-white" : "hover:bg-mid-gray/10"}`}
                      onClick={() => setImportTab("url")}
                    >
                      {t("settings.advanced.customWords.importUrl")}
                    </button>
                    <button
                      className={`px-3 py-1 text-xs rounded-md cursor-pointer transition-colors ${importTab === "ai" ? "bg-background-ui text-white" : "hover:bg-mid-gray/10"}`}
                      onClick={() => setImportTab("ai")}
                    >
                      {t("settings.advanced.customWords.generateAI")}
                    </button>
                  </div>

                  {importTab === "paste" && (
                    <div className="flex flex-col gap-2">
                      <textarea
                        className="w-full px-2 py-1 text-sm bg-mid-gray/10 border border-mid-gray/80 rounded-md resize-none focus:outline-none focus:border-logo-primary hover:border-logo-primary transition-colors"
                        rows={4}
                        value={pasteText}
                        onChange={(e) => setPasteText(e.target.value)}
                        placeholder={t(
                          "settings.advanced.customWords.importPlaceholder",
                        )}
                      />
                      <Button
                        onClick={handleAddPaste}
                        disabled={parsedPasteWords.length === 0}
                        variant="primary"
                        size="sm"
                      >
                        {t("settings.advanced.customWords.addNWords_other", {
                          count: parsedPasteWords.length,
                        })}
                      </Button>
                    </div>
                  )}

                  {importTab === "url" && (
                    <div className="flex flex-col gap-2">
                      <div className="flex gap-2">
                        <Input
                          type="url"
                          className="flex-1"
                          value={urlText}
                          onChange={(e) => setUrlText(e.target.value)}
                          placeholder={t(
                            "settings.advanced.customWords.importUrlPlaceholder",
                          )}
                          variant="compact"
                          disabled={urlFetching}
                          onKeyDown={(e) => {
                            if (e.key === "Enter") {
                              e.preventDefault();
                              handleFetchUrl();
                            }
                          }}
                        />
                        <Button
                          onClick={handleFetchUrl}
                          disabled={!urlText.trim() || urlFetching}
                          variant="primary"
                          size="sm"
                        >
                          {urlFetching
                            ? t("settings.advanced.customWords.fetching")
                            : t("settings.advanced.customWords.fetch")}
                        </Button>
                      </div>
                      {urlWords !== null && (
                        <div className="flex flex-col gap-1">
                          <p className="text-xs text-mid-gray">
                            {t("settings.advanced.customWords.addNWords_other", {
                              count: urlWords.length,
                            })}
                          </p>
                          <Button
                            onClick={handleAddUrl}
                            disabled={urlWords.length === 0}
                            variant="primary"
                            size="sm"
                          >
                            {t("settings.advanced.customWords.addNWords_other", {
                              count: urlWords.length,
                            })}
                          </Button>
                        </div>
                      )}
                    </div>
                  )}

                  {importTab === "ai" && (
                    <div className="flex flex-col gap-2">
                      <p className="text-xs text-mid-gray">
                        {t("settings.advanced.customWords.copyPromptHint")}
                      </p>
                      <Button
                        onClick={handleCopyAiPrompt}
                        variant="primary"
                        size="sm"
                      >
                        {t("settings.advanced.customWords.copyPrompt")}
                      </Button>
                    </div>
                  )}
                </div>
              )}
            </div>

            {/* Browse word lists section */}
            <div className="border border-mid-gray/20 rounded-lg overflow-hidden">
              <button
                className="w-full flex items-center justify-between px-3 py-2 text-sm font-medium hover:bg-mid-gray/10 transition-colors cursor-pointer"
                onClick={handleBrowseOpen}
              >
                <span>{t("settings.advanced.customWords.browseLists")}</span>
                <svg
                  className={`w-4 h-4 transition-transform ${browseOpen ? "rotate-180" : ""}`}
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

              {browseOpen && (
                <div className="border-t border-mid-gray/20 p-3 flex flex-col gap-2">
                  {listsLoading && (
                    <p className="text-xs text-mid-gray">
                      {t("settings.advanced.customWords.loadingLists")}
                    </p>
                  )}
                  {lists !== null &&
                    lists.map((meta) => (
                      <div
                        key={meta.id}
                        className="border border-mid-gray/20 rounded-md overflow-hidden"
                      >
                        <button
                          className="w-full flex items-center justify-between px-3 py-2 text-sm hover:bg-mid-gray/10 transition-colors cursor-pointer"
                          onClick={() => handleExpandList(meta)}
                        >
                          <div className="flex items-center gap-2 text-left">
                            <span className="font-medium">{meta.name}</span>
                            <span className="text-xs bg-mid-gray/20 px-1.5 py-0.5 rounded">
                              {meta.wordCount}
                            </span>
                          </div>
                          <svg
                            className={`w-3.5 h-3.5 flex-shrink-0 transition-transform ${expandedList === meta.id ? "rotate-180" : ""}`}
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

                        {expandedList === meta.id && (
                          <div className="border-t border-mid-gray/20 px-3 py-2 flex flex-col gap-2">
                            <p className="text-xs text-mid-gray">
                              {meta.description}
                            </p>
                            {listLoading[meta.id] && (
                              <p className="text-xs text-mid-gray">
                                {t("settings.advanced.customWords.fetching")}
                              </p>
                            )}
                            {listWords[meta.id] && (
                              <>
                                <p className="text-xs text-mid-gray">
                                  {listWords[meta.id]
                                    .slice(0, 10)
                                    .join(", ")}
                                  {listWords[meta.id].length > 10 && "…"}
                                </p>
                                <Button
                                  onClick={() => handleAddListWords(meta.id)}
                                  variant="primary-soft"
                                  size="sm"
                                >
                                  {t("settings.advanced.customWords.addList")}
                                </Button>
                              </>
                            )}
                          </div>
                        )}
                      </div>
                    ))}
                </div>
              )}
            </div>
          </div>
        </SettingContainer>

        {customWords.length > 0 && (
          <div
            className={`px-4 p-2 ${grouped ? "" : "rounded-lg border border-mid-gray/20"} flex flex-col gap-2`}
          >
            <div className="flex justify-end gap-2">
              <Button
                onClick={() => {
                  const content = customWords.join("\n");
                  const blob = new Blob([content], { type: "text/plain" });
                  const url = URL.createObjectURL(blob);
                  const a = document.createElement("a");
                  a.href = url;
                  a.download = "custom-words.txt";
                  a.click();
                  URL.revokeObjectURL(url);
                }}
                variant="ghost"
                size="sm"
              >
                {t("settings.advanced.customWords.exportWords")}
              </Button>
              <Button
                onClick={() => updateSetting("custom_words", [])}
                disabled={isUpdating("custom_words")}
                variant="danger-ghost"
                size="sm"
              >
                {t("settings.advanced.customWords.clearAll")}
              </Button>
            </div>
            <div className="flex flex-wrap gap-1">
            {customWords.map((word) => (
              <Button
                key={word}
                onClick={() => handleRemoveWord(word)}
                disabled={isUpdating("custom_words")}
                variant="secondary"
                size="sm"
                className="inline-flex items-center gap-1 cursor-pointer"
                aria-label={t("settings.advanced.customWords.remove", { word })}
              >
                <span>{word}</span>
                <svg
                  className="w-3 h-3"
                  fill="none"
                  stroke="currentColor"
                  viewBox="0 0 24 24"
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
          </div>
        )}
      </>
    );
  },
);

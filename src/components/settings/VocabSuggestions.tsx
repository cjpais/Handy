import React, { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Check, X, RefreshCw } from "lucide-react";
import { SettingContainer } from "../ui/SettingContainer";
import { Button } from "../ui/Button";
import { commands, events, type VocabSuggestion } from "@/bindings";

interface VocabSuggestionsProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

/** Learned-vocabulary suggestions mined from transcription history. */
export const VocabSuggestions: React.FC<VocabSuggestionsProps> = ({
  descriptionMode = "tooltip",
  grouped = false,
}) => {
  const { t } = useTranslation();
  const [suggestions, setSuggestions] = useState<VocabSuggestion[]>([]);
  const [scanning, setScanning] = useState(false);

  const refresh = useCallback(async () => {
    setSuggestions(await commands.getVocabSuggestions());
  }, []);

  useEffect(() => {
    refresh();
    const unlisten = events.vocabSuggestionsUpdated.listen(() => {
      setScanning(false);
      refresh();
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, [refresh]);

  const resolve = async (word: string, accept: boolean) => {
    await commands.resolveVocabSuggestion(word, accept);
    refresh();
  };

  const scanNow = async () => {
    setScanning(true);
    await commands.runVocabScanNow();
    // If the scan bails early (nothing new / provider down) no event fires;
    // clear the spinner after a grace period either way.
    setTimeout(() => setScanning(false), 15000);
  };

  return (
    <SettingContainer
      title={t("settings.vocabSuggestions.title")}
      description={t("settings.vocabSuggestions.description")}
      descriptionMode={descriptionMode}
      grouped={grouped}
      layout="stacked"
    >
      <div className="flex flex-col gap-2">
        {suggestions.length === 0 ? (
          <p className="text-xs text-muted-foreground">
            {t("settings.vocabSuggestions.empty")}
          </p>
        ) : (
          <div className="flex flex-wrap gap-2">
            {suggestions.map((s) => (
              <span
                key={s.word}
                className="inline-flex items-center gap-1 rounded-full bg-muted px-2 py-0.5 text-xs"
                title={t("settings.vocabSuggestions.chipTooltip", {
                  kind: s.kind,
                  count: s.evidence_count,
                })}
              >
                {s.word}
                <button
                  className="text-accent hover:text-foreground cursor-pointer"
                  onClick={() => resolve(s.word, true)}
                  aria-label={t("settings.vocabSuggestions.add")}
                >
                  <Check className="w-3.5 h-3.5" />
                </button>
                <button
                  className="text-muted-foreground hover:text-foreground cursor-pointer"
                  onClick={() => resolve(s.word, false)}
                  aria-label={t("settings.vocabSuggestions.dismiss")}
                >
                  <X className="w-3.5 h-3.5" />
                </button>
              </span>
            ))}
          </div>
        )}
        <div>
          <Button
            variant="secondary"
            size="sm"
            onClick={scanNow}
            disabled={scanning}
          >
            <span className="inline-flex items-center gap-1.5">
              <RefreshCw
                className={`w-3.5 h-3.5 ${scanning ? "animate-spin" : ""}`}
              />
              {scanning
                ? t("settings.vocabSuggestions.scanning")
                : t("settings.vocabSuggestions.scanNow")}
            </span>
          </Button>
        </div>
      </div>
    </SettingContainer>
  );
};

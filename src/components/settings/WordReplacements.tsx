import React, { useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import type { WordReplacement } from "@/bindings";
import { useSettings } from "../../hooks/useSettings";
import { Input } from "../ui/Input";
import { Button } from "../ui/Button";
import { SettingContainer } from "../ui/SettingContainer";

interface WordReplacementsProps {
  grouped?: boolean;
}

// Decorative separator between the "from" and "to" terms. Kept as a constant
// (not inline JSX text) so the i18next no-literal-string lint passes.
const ARROW = "→";

export const WordReplacements: React.FC<WordReplacementsProps> = React.memo(
  ({ grouped = false }) => {
    const { t } = useTranslation();
    const { getSetting, updateSetting, isUpdating } = useSettings();
    const [from, setFrom] = useState("");
    const [to, setTo] = useState("");
    const replacements = getSetting("word_replacements") || [];
    const updating = isUpdating("word_replacements");

    const handleAdd = () => {
      const trimmedFrom = from.trim();
      const trimmedTo = to.trim();
      if (!trimmedFrom) {
        return;
      }
      if (
        replacements.some(
          (r) => r.from.toLowerCase() === trimmedFrom.toLowerCase(),
        )
      ) {
        toast.error(
          t("settings.advanced.wordReplacements.duplicate", {
            word: trimmedFrom,
          }),
        );
        return;
      }
      const next: WordReplacement = { from: trimmedFrom, to: trimmedTo };
      updateSetting("word_replacements", [...replacements, next]);
      setFrom("");
      setTo("");
    };

    const handleRemove = (fromToRemove: string) => {
      updateSetting(
        "word_replacements",
        replacements.filter((r) => r.from !== fromToRemove),
      );
    };

    const handleKeyPress = (e: React.KeyboardEvent) => {
      if (e.key === "Enter") {
        e.preventDefault();
        handleAdd();
      }
    };

    return (
      <>
        <SettingContainer
          title={t("settings.advanced.wordReplacements.title")}
          description={t("settings.advanced.wordReplacements.description")}
          descriptionMode="inline"
          layout="stacked"
          grouped={grouped}
        >
          <div className="flex items-center gap-2">
            <Input
              type="text"
              className="flex-1 min-w-0"
              value={from}
              onChange={(e) => setFrom(e.target.value)}
              onKeyDown={handleKeyPress}
              placeholder={t(
                "settings.advanced.wordReplacements.fromPlaceholder",
              )}
              variant="compact"
              disabled={updating}
              aria-label={t("settings.advanced.wordReplacements.fromLabel")}
            />
            <span
              className="text-mid-gray shrink-0 select-none"
              aria-hidden="true"
            >
              {ARROW}
            </span>
            <Input
              type="text"
              className="flex-1 min-w-0"
              value={to}
              onChange={(e) => setTo(e.target.value)}
              onKeyDown={handleKeyPress}
              placeholder={t(
                "settings.advanced.wordReplacements.toPlaceholder",
              )}
              variant="compact"
              disabled={updating}
              aria-label={t("settings.advanced.wordReplacements.toLabel")}
            />
            <Button
              onClick={handleAdd}
              disabled={!from.trim() || updating}
              variant="primary"
              size="md"
              className="shrink-0"
            >
              {t("settings.advanced.wordReplacements.add")}
            </Button>
          </div>
        </SettingContainer>
        {replacements.length > 0 && (
          <div
            className={`px-4 p-2 ${grouped ? "" : "rounded-lg border border-mid-gray/20"} flex flex-wrap gap-1`}
          >
            {replacements.map((replacement) => (
              <Button
                key={replacement.from}
                onClick={() => handleRemove(replacement.from)}
                disabled={updating}
                variant="secondary"
                size="sm"
                className="inline-flex items-center gap-1.5 cursor-pointer"
                aria-label={t("settings.advanced.wordReplacements.remove", {
                  from: replacement.from,
                  to: replacement.to,
                })}
              >
                <span className="font-semibold">{replacement.from}</span>
                <span className="text-mid-gray" aria-hidden="true">
                  {ARROW}
                </span>
                <span className="font-semibold">
                  {replacement.to || (
                    <span className="italic font-normal text-mid-gray">
                      {t("settings.advanced.wordReplacements.emptyTarget")}
                    </span>
                  )}
                </span>
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
        )}
      </>
    );
  },
);

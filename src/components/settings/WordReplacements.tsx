import React, { useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { useSettings } from "../../hooks/useSettings";
import { Input } from "../ui/Input";
import { Button } from "../ui/Button";
import { SettingContainer } from "../ui/SettingContainer";

interface WordReplacementsProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const WordReplacements: React.FC<WordReplacementsProps> = React.memo(
  ({ descriptionMode = "tooltip", grouped = false }) => {
    const { t } = useTranslation();
    const { getSetting, updateSetting, isUpdating } = useSettings();
    const [newFrom, setNewFrom] = useState("");
    const [newTo, setNewTo] = useState("");
    const wordReplacements = getSetting("word_replacements") || [];

    const handleAdd = () => {
      const trimmedFrom = newFrom.trim().replace(/[<>"'&]/g, "");
      const trimmedTo = newTo.trim().replace(/[<>"'&]/g, "");
      if (!trimmedFrom || !trimmedTo || trimmedFrom.length > 100 || trimmedTo.length > 100) {
        return;
      }
      if (wordReplacements.some((r) => r.from.toLowerCase() === trimmedFrom.toLowerCase())) {
        toast.error(
          t("settings.advanced.wordReplacements.duplicate", { from: trimmedFrom }),
        );
        return;
      }
      updateSetting("word_replacements", [
        ...wordReplacements,
        { from: trimmedFrom, to: trimmedTo },
      ]);
      setNewFrom("");
      setNewTo("");
    };

    const handleRemove = (fromToRemove: string) => {
      updateSetting(
        "word_replacements",
        wordReplacements.filter((r) => r.from !== fromToRemove),
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
          descriptionMode={descriptionMode}
          grouped={grouped}
        >
          <div className="flex items-center gap-2">
            <Input
              type="text"
              className="max-w-28"
              value={newFrom}
              onChange={(e) => setNewFrom(e.target.value)}
              onKeyDown={handleKeyPress}
              placeholder={t("settings.advanced.wordReplacements.fromPlaceholder")}
              variant="compact"
              disabled={isUpdating("word_replacements")}
            />
            <span className="text-sm text-mid-gray">{"\u2192"}</span>
            <Input
              type="text"
              className="max-w-28"
              value={newTo}
              onChange={(e) => setNewTo(e.target.value)}
              onKeyDown={handleKeyPress}
              placeholder={t("settings.advanced.wordReplacements.toPlaceholder")}
              variant="compact"
              disabled={isUpdating("word_replacements")}
            />
            <Button
              onClick={handleAdd}
              disabled={
                !newFrom.trim() ||
                !newTo.trim() ||
                newFrom.trim().length > 100 ||
                newTo.trim().length > 100 ||
                isUpdating("word_replacements")
              }
              variant="primary"
              size="md"
            >
              {t("settings.advanced.wordReplacements.add")}
            </Button>
          </div>
        </SettingContainer>
        {wordReplacements.length > 0 && (
          <div
            className={`px-4 p-2 ${grouped ? "" : "rounded-lg border border-mid-gray/20"} flex flex-wrap gap-1`}
          >
            {wordReplacements.map((r) => (
              <Button
                key={r.from}
                onClick={() => handleRemove(r.from)}
                disabled={isUpdating("word_replacements")}
                variant="secondary"
                size="sm"
                className="inline-flex items-center gap-1 cursor-pointer"
                aria-label={t("settings.advanced.wordReplacements.remove", { from: r.from })}
              >
                <span>{r.from} {"\u2192"} {r.to}</span>
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

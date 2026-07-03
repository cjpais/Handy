import React, { useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { useSettings } from "../../hooks/useSettings";
import { Input } from "../ui/Input";
import { Button } from "../ui/Button";
import { SettingContainer } from "../ui/SettingContainer";
import type { Snippet } from "@/bindings";

interface SnippetsProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const Snippets: React.FC<SnippetsProps> = React.memo(
  ({ descriptionMode = "tooltip", grouped = false }) => {
    const { t } = useTranslation();
    const { getSetting, updateSetting, isUpdating } = useSettings();
    const [newTrigger, setNewTrigger] = useState("");
    const [newExpansion, setNewExpansion] = useState("");
    const snippets: Snippet[] = getSetting("snippets") || [];

    const trimmedTrigger = newTrigger.trim();
    const canAdd =
      trimmedTrigger.length > 0 &&
      trimmedTrigger.length <= 50 &&
      newExpansion.trim().length > 0 &&
      !isUpdating("snippets");

    const handleAdd = () => {
      if (!canAdd) return;
      if (
        snippets.some(
          (s) => s.trigger.toLowerCase() === trimmedTrigger.toLowerCase(),
        )
      ) {
        toast.error(
          t("settings.advanced.snippets.duplicate", { trigger: trimmedTrigger }),
        );
        return;
      }
      updateSetting("snippets", [
        ...snippets,
        { trigger: trimmedTrigger, expansion: newExpansion.trim() },
      ]);
      setNewTrigger("");
      setNewExpansion("");
    };

    const handleRemove = (trigger: string) => {
      updateSetting(
        "snippets",
        snippets.filter((s) => s.trigger !== trigger),
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
          title={t("settings.advanced.snippets.title")}
          description={t("settings.advanced.snippets.description")}
          descriptionMode={descriptionMode}
          grouped={grouped}
        >
          <div className="flex items-center gap-2">
            <Input
              type="text"
              className="max-w-32"
              value={newTrigger}
              onChange={(e) => setNewTrigger(e.target.value)}
              onKeyDown={handleKeyPress}
              placeholder={t("settings.advanced.snippets.triggerPlaceholder")}
              variant="compact"
              disabled={isUpdating("snippets")}
            />
            <Input
              type="text"
              className="max-w-40"
              value={newExpansion}
              onChange={(e) => setNewExpansion(e.target.value)}
              onKeyDown={handleKeyPress}
              placeholder={t("settings.advanced.snippets.expansionPlaceholder")}
              variant="compact"
              disabled={isUpdating("snippets")}
            />
            <Button
              onClick={handleAdd}
              disabled={!canAdd}
              variant="primary"
              size="md"
            >
              {t("settings.advanced.snippets.add")}
            </Button>
          </div>
        </SettingContainer>
        {snippets.length > 0 && (
          <div
            className={`px-4 p-2 ${grouped ? "" : "rounded-lg border border-mid-gray/20"} flex flex-wrap gap-1`}
          >
            {snippets.map((snippet) => (
              <Button
                key={snippet.trigger}
                onClick={() => handleRemove(snippet.trigger)}
                disabled={isUpdating("snippets")}
                variant="secondary"
                size="sm"
                className="inline-flex items-center gap-1 cursor-pointer"
                aria-label={t("settings.advanced.snippets.remove", {
                  trigger: snippet.trigger,
                })}
              >
                <span>
                  {snippet.trigger} → {snippet.expansion}
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

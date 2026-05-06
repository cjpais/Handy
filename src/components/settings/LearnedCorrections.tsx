import React, { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { commands, type LearnedCorrection } from "@/bindings";
import { Button } from "../ui/Button";
import { SettingContainer } from "../ui/SettingContainer";
import { Trash2, X, Brain } from "lucide-react";

interface LearnedCorrectionsProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const LearnedCorrections: React.FC<LearnedCorrectionsProps> =
  React.memo(({ descriptionMode = "tooltip", grouped = false }) => {
    const { t } = useTranslation();
    const [corrections, setCorrections] = useState<LearnedCorrection[]>([]);
    const [loading, setLoading] = useState(true);

    const loadCorrections = useCallback(async () => {
      try {
        const result = await commands.getLearnedCorrections();
        if (result.status === "ok") {
          setCorrections(result.data);
        }
      } catch (error) {
        console.error("Failed to load learned corrections:", error);
      } finally {
        setLoading(false);
      }
    }, []);

    useEffect(() => {
      loadCorrections();
    }, [loadCorrections]);

    const handleDelete = async (id: number) => {
      const prev = corrections;
      setCorrections((c) => c.filter((cor) => cor.id !== id));
      try {
        const result = await commands.deleteLearnedCorrection(id);
        if (result.status !== "ok") {
          setCorrections(prev);
        }
      } catch {
        setCorrections(prev);
      }
    };

    const handleClearAll = async () => {
      const prev = corrections;
      setCorrections([]);
      try {
        const result = await commands.clearLearnedCorrections();
        if (result.status !== "ok") {
          setCorrections(prev);
          toast.error(t("settings.advanced.learnedCorrections.clearError"));
        } else {
          toast.success(
            t("settings.advanced.learnedCorrections.cleared", {
              count: result.data,
            }),
          );
        }
      } catch {
        setCorrections(prev);
      }
    };

    return (
      <>
        <SettingContainer
          title={t("settings.advanced.learnedCorrections.title")}
          description={t("settings.advanced.learnedCorrections.description")}
          descriptionMode={descriptionMode}
          grouped={grouped}
        >
          <div className="flex items-center gap-2">
            {corrections.length > 0 && (
              <Button
                onClick={handleClearAll}
                variant="secondary"
                size="md"
                className="text-red-400 hover:text-red-300"
              >
                {t("settings.advanced.learnedCorrections.clearAll")}
              </Button>
            )}
          </div>
        </SettingContainer>
        {loading ? (
          <div className="px-4 py-2 text-xs text-text/40">
            {t("settings.advanced.learnedCorrections.loading")}
          </div>
        ) : corrections.length > 0 ? (
          <div
            className={`px-4 p-2 ${grouped ? "" : "rounded-lg border border-mid-gray/20"} flex flex-wrap gap-1`}
          >
            {corrections.map((correction) => (
              <Button
                key={correction.id}
                onClick={() => handleDelete(correction.id)}
                variant="secondary"
                size="sm"
                className="inline-flex items-center gap-1 cursor-pointer"
                aria-label={t("settings.advanced.learnedCorrections.remove", {
                  word: correction.original_word,
                })}
              >
                <span className="text-red-400/80 line-through">
                  {correction.original_word}
                </span>
                <span className="text-text/30">{"\u2192"}</span>
                <span className="text-green-400">
                  {correction.corrected_word}
                </span>
                <X className="w-3 h-3 ml-1" />
              </Button>
            ))}
          </div>
        ) : (
          <div className="px-4 py-2 text-xs text-text/40">
            {t("settings.advanced.learnedCorrections.empty")}
          </div>
        )}
      </>
    );
  });

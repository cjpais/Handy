import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { commands } from "@/bindings";
import { SettingContainer } from "@/components/ui/SettingContainer";
import { Textarea } from "@/components/ui/Textarea";
import { useSettings } from "@/hooks/useSettings";

/** "um,  like ,you know" -> ["um", "like", "you know"] */
export function parseFillerList(text: string): string[] {
  return text
    .split(/[,;\n]/)
    .map((w) => w.trim())
    .filter(Boolean);
}

/** Editable list of the words the Words tab counts as fillers. */
export const FillerWordsSettings: React.FC = () => {
  const { t } = useTranslation();
  const { settings, refreshSettings } = useSettings();
  const stored = (settings?.voxa?.filler_words ?? []).join(", ");
  const [draft, setDraft] = useState(stored);
  const [saving, setSaving] = useState(false);
  useEffect(() => setDraft(stored), [stored]);

  const save = async () => {
    const list = parseFillerList(draft);
    if (list.join(", ") === stored) return;
    setSaving(true);
    try {
      const result = await commands.voxaSetFillerWords(list);
      if (result.status === "error") toast.error(result.error);
      await refreshSettings();
    } finally {
      setSaving(false);
    }
  };

  return (
    <SettingContainer
      title={t("voxa.settings.fillers.title")}
      description={t("voxa.settings.fillers.description")}
      descriptionMode="tooltip"
      grouped={true}
      layout="stacked"
    >
      <Textarea
        variant="compact"
        className="w-full"
        rows={2}
        maxLength={600}
        value={draft}
        disabled={saving}
        placeholder={t("voxa.settings.fillers.placeholder")}
        onChange={(e) => setDraft(e.target.value)}
        onBlur={() => void save()}
        data-testid="filler-words-input"
      />
    </SettingContainer>
  );
};

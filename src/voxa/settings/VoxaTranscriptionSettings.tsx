import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { commands } from "@/bindings";
import { SettingContainer } from "@/components/ui/SettingContainer";
import { Textarea } from "@/components/ui/Textarea";
import { ToggleSwitch } from "@/components/ui/ToggleSwitch";
import { useSettings } from "@/hooks/useSettings";

/**
 * Voxa's two transcription settings: the context phrase handed to Whisper and
 * the case repair of dictionary words. Rendered inside the "Transcription"
 * group of the advanced screen.
 */
export const VoxaTranscriptionSettings: React.FC = () => {
  const { t } = useTranslation();
  const { settings, refreshSettings } = useSettings();
  const stored = settings?.voxa?.initial_prompt ?? "";
  const fixCase = settings?.voxa?.fix_custom_word_case ?? true;

  const [draft, setDraft] = useState(stored);
  const [saving, setSaving] = useState(false);
  useEffect(() => setDraft(stored), [stored]);

  const savePrompt = async () => {
    if (draft.trim() === stored.trim()) return;
    setSaving(true);
    try {
      const result = await commands.voxaSetInitialPrompt(draft);
      if (result.status === "error") toast.error(result.error);
      await refreshSettings();
    } finally {
      setSaving(false);
    }
  };

  const toggleFixCase = async (enabled: boolean) => {
    setSaving(true);
    try {
      const result = await commands.voxaSetFixCustomWordCase(enabled);
      if (result.status === "error") toast.error(result.error);
      await refreshSettings();
    } finally {
      setSaving(false);
    }
  };

  return (
    <>
      <SettingContainer
        title={t("voxa.settings.initialPrompt.title")}
        description={t("voxa.settings.initialPrompt.description")}
        descriptionMode="tooltip"
        grouped={true}
        layout="stacked"
      >
        <Textarea
          variant="compact"
          className="w-full"
          rows={2}
          maxLength={500}
          value={draft}
          disabled={saving}
          placeholder={t("voxa.settings.initialPrompt.placeholder")}
          onChange={(e) => setDraft(e.target.value)}
          onBlur={() => void savePrompt()}
        />
      </SettingContainer>
      <ToggleSwitch
        checked={fixCase}
        onChange={(enabled) => void toggleFixCase(enabled)}
        isUpdating={saving}
        label={t("voxa.settings.fixCase.title")}
        description={t("voxa.settings.fixCase.description")}
        descriptionMode="tooltip"
        grouped={true}
      />
    </>
  );
};

export default VoxaTranscriptionSettings;

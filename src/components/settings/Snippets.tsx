import React, { useId, useState } from "react";
import { useTranslation } from "react-i18next";
import type { Snippet } from "@/bindings";
import { useSettings } from "../../hooks/useSettings";
import { Button } from "../ui/Button";
import { Input } from "../ui/Input";
import { ToggleSwitch } from "../ui/ToggleSwitch";
import { SettingsGroup } from "../ui/SettingsGroup";

const normalizePhrase = (phrase: string) => phrase.trim().replace(/\s+/gu, " ");
const phraseKey = (phrase: string) =>
  normalizePhrase(phrase).toLowerCase().replace(/ς/g, "σ");

export const SnippetsSettings: React.FC = () => {
  const { t } = useTranslation();
  return (
    <div className="max-w-3xl w-full mx-auto">
      <SettingsGroup title={t("settings.advanced.snippets.title")}>
        <Snippets />
      </SettingsGroup>
    </div>
  );
};

export const Snippets: React.FC = () => {
  const { t } = useTranslation();
  const { getSetting, updateSetting, isLoading, isUpdating } = useSettings();
  const fieldId = useId();
  const [editingId, setEditingId] = useState<string | null>(null);
  const [phrase, setPhrase] = useState("");
  const [replacement, setReplacement] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const snippets = getSetting("snippets") ?? [];
  const enabled = getSetting("snippets_enabled") ?? false;
  const busy =
    isLoading ||
    saving ||
    isUpdating("snippets") ||
    isUpdating("snippets_enabled");

  const resetDraft = () => {
    setEditingId(null);
    setPhrase("");
    setReplacement("");
    setError(null);
  };

  const persist = async (next: Snippet[]) => {
    setSaving(true);
    setError(null);
    try {
      await updateSetting("snippets", next);
      return true;
    } catch {
      setError(t("settings.advanced.snippets.saveFailed"));
      return false;
    } finally {
      setSaving(false);
    }
  };

  const saveDraft = async (event: React.FormEvent) => {
    event.preventDefault();
    if (busy) return;
    const normalized = normalizePhrase(phrase);
    if (!normalized || Array.from(normalized).length > 100) {
      setError(t("settings.advanced.snippets.invalidPhrase"));
      return;
    }
    if (!/[\p{L}\p{N}]/u.test(normalized)) {
      setError(t("settings.advanced.snippets.phraseNeedsWord"));
      return;
    }
    if (!editingId && snippets.length >= 500) {
      setError(t("settings.advanced.snippets.limitReached"));
      return;
    }
    if (!replacement.trim() || Array.from(replacement).length > 4000) {
      setError(t("settings.advanced.snippets.invalidReplacement"));
      return;
    }
    if (
      snippets.some(
        (snippet) =>
          snippet.id !== editingId &&
          phraseKey(snippet.phrase) === phraseKey(normalized),
      )
    ) {
      setError(t("settings.advanced.snippets.duplicate"));
      return;
    }
    if (editingId && !snippets.some((snippet) => snippet.id === editingId)) {
      setError(t("settings.advanced.snippets.missing"));
      return;
    }
    const next = editingId
      ? snippets.map((snippet) =>
          snippet.id === editingId
            ? { ...snippet, phrase: normalized, replacement }
            : snippet,
        )
      : [
          ...snippets,
          {
            id: crypto.randomUUID(),
            phrase: normalized,
            replacement,
            enabled: true,
          },
        ];
    if (await persist(next)) resetDraft();
  };

  const toggleEnabled = async (next: boolean) => {
    setSaving(true);
    setError(null);
    try {
      await updateSetting("snippets_enabled", next);
    } catch {
      setError(t("settings.advanced.snippets.saveFailed"));
    } finally {
      setSaving(false);
    }
  };

  return (
    <div>
      <ToggleSwitch
        checked={enabled}
        onChange={(next) => void toggleEnabled(next)}
        disabled={busy}
        label={t("settings.advanced.snippets.title")}
        description={t("settings.advanced.snippets.description")}
        descriptionMode="inline"
        grouped
      />
      <div className="px-4 pb-4 space-y-4">
        <p className="text-sm text-text/70">
          {t("settings.advanced.snippets.matching")}
        </p>
        {snippets.length === 0 ? (
          <p className="text-sm text-text/70">
            {t("settings.advanced.snippets.empty")}
          </p>
        ) : (
          <ul className="space-y-2">
            {snippets.map((snippet) => (
              <li
                key={snippet.id}
                className="rounded-lg border border-line p-3 space-y-2"
              >
                <div className="flex flex-wrap items-center gap-2">
                  <label className="flex items-center gap-2 min-w-0 flex-1">
                    <input
                      type="checkbox"
                      checked={snippet.enabled}
                      disabled={busy}
                      aria-label={t("settings.advanced.snippets.enableRow", {
                        phrase: snippet.phrase,
                      })}
                      onChange={(event) =>
                        void persist(
                          snippets.map((row) =>
                            row.id === snippet.id
                              ? { ...row, enabled: event.target.checked }
                              : row,
                          ),
                        )
                      }
                    />
                    <span className="font-medium break-words">
                      {snippet.phrase}
                    </span>
                  </label>
                  <Button
                    type="button"
                    variant="secondary"
                    size="sm"
                    disabled={busy || editingId !== null}
                    aria-label={t("settings.advanced.snippets.editRow", {
                      phrase: snippet.phrase,
                    })}
                    onClick={() => {
                      setEditingId(snippet.id);
                      setPhrase(snippet.phrase);
                      setReplacement(snippet.replacement);
                      setError(null);
                    }}
                  >
                    {t("settings.advanced.snippets.edit")}
                  </Button>
                  <Button
                    type="button"
                    variant="danger-ghost"
                    size="sm"
                    disabled={busy || editingId !== null}
                    aria-label={t("settings.advanced.snippets.removeRow", {
                      phrase: snippet.phrase,
                    })}
                    onClick={() =>
                      void persist(
                        snippets.filter((row) => row.id !== snippet.id),
                      )
                    }
                  >
                    {t("settings.advanced.snippets.remove")}
                  </Button>
                </div>
                <p className="text-sm text-text/70 whitespace-pre-wrap break-words max-h-32 overflow-auto">
                  {snippet.replacement}
                </p>
              </li>
            ))}
          </ul>
        )}
        <form onSubmit={(event) => void saveDraft(event)} className="space-y-3">
          <h3 className="text-sm font-medium">
            {t(
              editingId
                ? "settings.advanced.snippets.editTitle"
                : "settings.advanced.snippets.addTitle",
            )}
          </h3>
          <div className="space-y-1">
            <label htmlFor={`${fieldId}-phrase`} className="text-sm">
              {t("settings.advanced.snippets.phrase")}
            </label>
            <Input
              id={`${fieldId}-phrase`}
              value={phrase}
              disabled={busy}
              className="w-full"
              placeholder={t("settings.advanced.snippets.phrasePlaceholder")}
              onChange={(event) => setPhrase(event.target.value)}
            />
          </div>
          <div className="space-y-1">
            <label htmlFor={`${fieldId}-replacement`} className="text-sm">
              {t("settings.advanced.snippets.replacement")}
            </label>
            <textarea
              id={`${fieldId}-replacement`}
              value={replacement}
              disabled={busy}
              rows={4}
              className="w-full rounded-md border border-line-strong bg-wash px-3 py-2 text-sm disabled:opacity-60 focus:outline-none focus:border-logo-primary"
              placeholder={t(
                "settings.advanced.snippets.replacementPlaceholder",
              )}
              onChange={(event) => setReplacement(event.target.value)}
            />
          </div>
          <div className="flex items-center gap-2">
            <Button type="submit" disabled={busy}>
              {t(
                editingId
                  ? "settings.advanced.snippets.save"
                  : "settings.advanced.snippets.add",
              )}
            </Button>
            {(editingId || phrase || replacement) && (
              <Button
                type="button"
                variant="secondary"
                disabled={busy}
                onClick={resetDraft}
              >
                {t("settings.advanced.snippets.cancel")}
              </Button>
            )}
          </div>
        </form>
        {error && (
          <p role="alert" className="text-sm text-error">
            {error}
          </p>
        )}
      </div>
    </div>
  );
};

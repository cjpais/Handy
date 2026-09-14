import React, { useEffect, useId, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { useSettings } from "../../../hooks/useSettings";
import { Button } from "../../ui/Button";
import { Input } from "../../ui/Input";
import {
  type DictionaryRule,
  normalizeReplacementFrom,
  normalizeReplacementTo,
  validateReplacement,
} from "./dictionaryFile";

interface DictionaryReplacementsProps {
  /** Current dictionary search query (manages/persists nothing itself). */
  filter: string;
}

/**
 * Replacements section of the Dictionary page.
 *
 * Rules are edited inline and persisted through useSettings() on blur/Enter —
 * never on each keystroke. Stored rules are keyed by array index (no ids): the
 * draft/commit flow below keeps indices stable by only appending or removing
 * whole entries.
 *
 * `saved` is what is persisted, `rows` is what is being edited. They stay
 * index-aligned so every payload is derived from `saved` plus exactly one
 * acted-upon change: another row's uncommitted draft can never leak into a
 * write, and a failed/invalid draft only reverts its own row.
 */
export const DictionaryReplacements: React.FC<DictionaryReplacementsProps> = ({
  filter,
}) => {
  const { t } = useTranslation();
  const { getSetting, updateSetting, isUpdating } = useSettings();
  const savedRules = (getSetting("dictionary_replacements") ||
    []) as DictionaryRule[];
  const savedSignature = JSON.stringify(savedRules);
  const readPersisted = () => JSON.parse(savedSignature) as DictionaryRule[];
  const [saved, setSaved] = useState<DictionaryRule[]>(readPersisted);
  const [rows, setRows] = useState<DictionaryRule[]>(readPersisted);
  const [newFrom, setNewFrom] = useState("");
  const [newTo, setNewTo] = useState("");
  const addFromId = useId();
  const addToId = useId();
  const updating = isUpdating("dictionary_replacements");
  // Signature of this component's last write. The sync effect uses it to tell
  // our own update apart from an external one, so other rows keep their drafts.
  const ownWriteSignature = useRef<string | null>(null);

  // Re-sync the persisted copy whenever the value changes from elsewhere
  // (import, reset, another settings page); an external change also discards
  // drafts, because the rows they were drafts of no longer exist.
  useEffect(() => {
    const persisted = JSON.parse(savedSignature) as DictionaryRule[];
    setSaved(persisted);
    if (ownWriteSignature.current === savedSignature) {
      ownWriteSignature.current = null;
      return;
    }
    setRows(persisted);
  }, [savedSignature]);

  const visibleRows = useMemo(() => {
    const needle = filter.trim().toLowerCase();
    return rows
      .map((rule, index) => ({ rule, index }))
      .filter(
        ({ rule }) =>
          !needle ||
          rule.from.toLowerCase().includes(needle) ||
          rule.to.toLowerCase().includes(needle),
      );
  }, [rows, filter]);

  const focusAddInput = () => {
    document.getElementById(addFromId)?.focus();
  };

  /** Revert only the offending row; every other row keeps its draft. */
  const revertRow = (index: number) => {
    setRows((current) =>
      current.map((rule, ruleIndex) =>
        ruleIndex === index ? (saved[index] ?? rule) : rule,
      ),
    );
  };

  /** Persist a payload computed from `saved`, then adopt it as the new baseline. */
  const persist = async (next: DictionaryRule[]) => {
    ownWriteSignature.current = JSON.stringify(next);
    await updateSetting("dictionary_replacements", next);
    setSaved(next);
  };

  const commitRow = async (index: number, from: string, to: string) => {
    const error = validateReplacement(from, to, saved, index);
    if (error === "duplicate") {
      toast.error(
        t("settings.dictionary.replacements.duplicate", {
          from: normalizeReplacementFrom(from),
        }),
      );
      revertRow(index);
      return;
    }
    if (error) {
      toast.error(t("settings.dictionary.replacements.invalid"));
      revertRow(index);
      return;
    }
    const nextFrom = normalizeReplacementFrom(from);
    const nextTo = normalizeReplacementTo(to);
    setRows((current) =>
      current.map((rule, ruleIndex) =>
        ruleIndex === index ? { from: nextFrom, to: nextTo } : rule,
      ),
    );
    if (nextFrom === saved[index]?.from && nextTo === saved[index]?.to) return;
    const next = saved.map((rule, ruleIndex) =>
      ruleIndex === index ? { from: nextFrom, to: nextTo } : rule,
    );
    await persist(next);
  };

  const updateRowDraft = (index: number, from: string, to: string) => {
    setRows(
      rows.map((rule, ruleIndex) =>
        ruleIndex === index ? { from, to } : rule,
      ),
    );
  };

  const handleDelete = async (index: number) => {
    const next = saved.filter((_, ruleIndex) => ruleIndex !== index);
    setRows((current) => current.filter((_, ruleIndex) => ruleIndex !== index));
    await persist(next);
    // The clicked row (and its button) is gone after this render; keep
    // keyboard focus inside the section instead of dropping it on <body>.
    // Focused after the write resolves: the inputs are `disabled` while
    // `updating`, and a disabled element cannot hold focus.
    focusAddInput();
  };

  const handleAdd = async () => {
    const error = validateReplacement(newFrom, newTo, saved);
    if (error === "duplicate") {
      toast.error(
        t("settings.dictionary.replacements.duplicate", {
          from: normalizeReplacementFrom(newFrom),
        }),
      );
      return;
    }
    if (error) {
      toast.error(t("settings.dictionary.replacements.invalid"));
      return;
    }
    const added = {
      from: normalizeReplacementFrom(newFrom),
      to: normalizeReplacementTo(newTo),
    };
    const next = [...saved, added];
    setRows((current) => [...current, added]);
    setNewFrom("");
    setNewTo("");
    await persist(next);
    focusAddInput();
  };

  const handleAddKeyDown = (e: React.KeyboardEvent) => {
    if (e.key !== "Enter") return;
    e.preventDefault();
    // Enter on the "heard" field advances to the replacement field instead of
    // failing validation halfway through the pair.
    if (normalizeReplacementFrom(newFrom) && !normalizeReplacementTo(newTo)) {
      document.getElementById(addToId)?.focus();
      return;
    }
    void handleAdd();
  };

  return (
    <div className="space-y-2">
      <div className="px-4 flex items-baseline justify-between gap-4">
        <div>
          <h2 className="text-xs font-medium text-mid-gray uppercase tracking-wide">
            {t("settings.dictionary.replacements.title")}
          </h2>
          <p className="text-xs text-mid-gray mt-1">
            {t("settings.dictionary.replacements.description")}
          </p>
        </div>
        <span className="shrink-0 text-xs text-mid-gray">
          {t("settings.dictionary.replacements.count", {
            count: visibleRows.length,
          })}
        </span>
      </div>
      <div className="bg-background border border-mid-gray/20 rounded-lg overflow-visible">
        <div className="divide-y divide-mid-gray/20">
          {rows.length === 0 ? (
            <p className="px-4 py-3 text-sm text-mid-gray">
              {t("settings.dictionary.replacements.empty")}
            </p>
          ) : visibleRows.length === 0 ? (
            <p className="px-4 py-3 text-sm text-mid-gray">
              {t("settings.dictionary.search.noResults")}
            </p>
          ) : (
            visibleRows.map(({ rule, index }) => (
              <div key={index} className="p-2 flex items-center gap-2">
                <Input
                  type="text"
                  variant="compact"
                  className="flex-1 min-w-0"
                  value={rule.from}
                  onChange={(e) =>
                    updateRowDraft(index, e.target.value, rule.to)
                  }
                  onBlur={(e) => void commitRow(index, e.target.value, rule.to)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter") {
                      e.preventDefault();
                      void commitRow(index, rule.from, rule.to);
                    }
                    if (e.key === "Escape") revertRow(index);
                  }}
                  placeholder={t("settings.dictionary.replacements.from")}
                  aria-label={t("settings.dictionary.replacements.from")}
                  disabled={updating}
                />
                <span aria-hidden="true" className="shrink-0 text-mid-gray">
                  →
                </span>
                <Input
                  type="text"
                  variant="compact"
                  className="flex-1 min-w-0"
                  value={rule.to}
                  onChange={(e) =>
                    updateRowDraft(index, rule.from, e.target.value)
                  }
                  onBlur={(e) =>
                    void commitRow(index, rule.from, e.target.value)
                  }
                  onKeyDown={(e) => {
                    if (e.key === "Enter") {
                      e.preventDefault();
                      void commitRow(index, rule.from, rule.to);
                    }
                    if (e.key === "Escape") revertRow(index);
                  }}
                  placeholder={t("settings.dictionary.replacements.to")}
                  aria-label={t("settings.dictionary.replacements.to")}
                  disabled={updating}
                />
                <Button
                  onClick={() => void handleDelete(index)}
                  disabled={updating}
                  variant="danger-ghost"
                  size="sm"
                  aria-label={t("settings.dictionary.replacements.delete", {
                    from: rule.from,
                  })}
                >
                  <svg
                    className="w-3.5 h-3.5"
                    fill="none"
                    stroke="currentColor"
                    viewBox="0 0 24 24"
                    aria-hidden="true"
                  >
                    <path
                      strokeLinecap="round"
                      strokeLinejoin="round"
                      strokeWidth={2}
                      d="M6 18L18 6M6 6l12 12"
                    />
                  </svg>
                </Button>
              </div>
            ))
          )}
          <div className="p-2 flex flex-col gap-2 sm:flex-row sm:items-center">
            <Input
              id={addFromId}
              type="text"
              variant="compact"
              className="flex-1 min-w-0"
              value={newFrom}
              onChange={(e) => setNewFrom(e.target.value)}
              onKeyDown={handleAddKeyDown}
              placeholder={t("settings.dictionary.replacements.from")}
              aria-label={t("settings.dictionary.replacements.from")}
              disabled={updating}
            />
            <span aria-hidden="true" className="shrink-0 text-mid-gray">
              →
            </span>
            <Input
              id={addToId}
              type="text"
              variant="compact"
              className="flex-1 min-w-0"
              value={newTo}
              onChange={(e) => setNewTo(e.target.value)}
              onKeyDown={handleAddKeyDown}
              placeholder={t("settings.dictionary.replacements.to")}
              aria-label={t("settings.dictionary.replacements.to")}
              disabled={updating}
            />
            <Button
              onClick={() => void handleAdd()}
              disabled={
                !normalizeReplacementFrom(newFrom) ||
                !normalizeReplacementTo(newTo) ||
                updating
              }
              variant="primary"
              size="md"
            >
              {t("settings.dictionary.replacements.add")}
            </Button>
          </div>
        </div>
      </div>
    </div>
  );
};

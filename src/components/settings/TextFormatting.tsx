import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { useSettings } from "../../hooks/useSettings";
import { ToggleSwitch } from "../ui/ToggleSwitch";
import { Dropdown } from "../ui/Dropdown";
import { SettingContainer } from "../ui/SettingContainer";
import type {
  TextFormatting as Formatting,
  InitialCapitalization,
  PeriodHandling,
} from "@/bindings";

export const TextFormatting: React.FC = () => {
  const { t } = useTranslation();
  const { getSetting, updateSetting, isUpdating } = useSettings();
  const config = getSetting("text_formatting");
  const [draft, setDraft] = useState<Formatting["replacements"]>([]);
  const [error, setError] = useState(false);
  const savedRules = JSON.stringify(config?.replacements ?? []);
  useEffect(() => {
    setDraft(JSON.parse(savedRules) as Formatting["replacements"]);
  }, [savedRules]);
  if (!config) return null;
  const busy = isUpdating("text_formatting");
  const update = (patch: Partial<Formatting>) =>
    updateSetting("text_formatting", { ...config, ...patch });
  const key = "settings.advanced.textFormatting";
  const save = async () => {
    try {
      const phrases = draft.map((row) => row.phrase.trim().toLowerCase());
      const bytes = new TextEncoder();
      if (
        draft.length > 100 ||
        new Set(phrases).size !== phrases.length ||
        draft.some(
          (row) =>
            !row.phrase.trim() ||
            bytes.encode(row.phrase.trim()).length > 200 ||
            bytes.encode(row.replacement).length > 100,
        )
      )
        throw new Error();
      await update({ replacements: draft });
      setError(false);
    } catch {
      setError(true);
    }
  };
  return (
    <>
      <ToggleSwitch
        checked={config.enabled}
        onChange={(enabled) => update({ enabled })}
        isUpdating={busy}
        label={t(`${key}.title`)}
        description={t(`${key}.description`)}
        grouped
      />
      {config.enabled && (
        <>
          <ToggleSwitch
            checked={config.spoken_punctuation}
            onChange={(spoken_punctuation) => update({ spoken_punctuation })}
            isUpdating={busy}
            label={t(`${key}.spoken`)}
            description={t(`${key}.spokenDescription`)}
            grouped
          />
          <SettingContainer
            title={t(`${key}.capitalization`)}
            description={t(`${key}.capitalizationDescription`)}
            grouped
          >
            <Dropdown
              selectedValue={config.initial_capitalization}
              disabled={busy}
              options={["keep", "lower", "upper"].map((value) => ({
                value,
                label: t(`${key}.capitalizationOptions.${value}`),
              }))}
              onSelect={(value) =>
                update({
                  initial_capitalization: value as InitialCapitalization,
                })
              }
            />
          </SettingContainer>
          <SettingContainer
            title={t(`${key}.periods`)}
            description={t(`${key}.periodsDescription`)}
            grouped
          >
            <Dropdown
              selectedValue={config.periods}
              disabled={busy}
              options={["keep", "remove_final", "remove_sentence"].map(
                (value) => ({
                  value,
                  label: t(`${key}.periodOptions.${value}`),
                }),
              )}
              onSelect={(value) => update({ periods: value as PeriodHandling })}
            />
          </SettingContainer>
          {config.spoken_punctuation && (
            <SettingContainer
              title={t(`${key}.replacements`)}
              description={t(`${key}.replacementsDescription`)}
              descriptionMode="inline"
              layout="stacked"
              grouped
            >
              <div className="space-y-2">
                {draft.map((row, index) => (
                  <div key={index} className="flex gap-2">
                    <input
                      aria-label={t(`${key}.phraseLabel`, {
                        number: index + 1,
                      })}
                      className="min-w-0 flex-1 p-2 border border-mid-gray/30 rounded bg-transparent"
                      value={row.phrase}
                      disabled={busy}
                      onChange={(event) =>
                        setDraft(
                          draft.map((item, i) =>
                            i === index
                              ? { ...item, phrase: event.target.value }
                              : item,
                          ),
                        )
                      }
                    />
                    <input
                      aria-label={t(`${key}.replacementLabel`, {
                        number: index + 1,
                      })}
                      className="min-w-0 w-28 p-2 border border-mid-gray/30 rounded bg-transparent"
                      value={row.replacement.replace(/\n/g, "\\n")}
                      disabled={busy}
                      onChange={(event) =>
                        setDraft(
                          draft.map((item, i) =>
                            i === index
                              ? {
                                  ...item,
                                  replacement: event.target.value.replace(
                                    /\\n/g,
                                    "\n",
                                  ),
                                }
                              : item,
                          ),
                        )
                      }
                    />
                    <button
                      aria-label={t(`${key}.removeLabel`, {
                        number: index + 1,
                      })}
                      className="px-2"
                      disabled={busy}
                      onClick={() =>
                        setDraft(draft.filter((_, i) => i !== index))
                      }
                    >
                      {t(`${key}.remove`)}
                    </button>
                  </div>
                ))}
                <button
                  className="px-3 py-1 rounded border border-mid-gray/30"
                  disabled={busy || draft.length >= 100}
                  onClick={() =>
                    setDraft([...draft, { phrase: "", replacement: "" }])
                  }
                >
                  {t(`${key}.add`)}
                </button>
              </div>
              {error && (
                <p role="alert" className="text-red-500 text-sm">
                  {t(`${key}.invalid`)}
                </p>
              )}
              <button
                className="px-3 py-1 rounded border border-mid-gray/30"
                disabled={busy}
                onClick={save}
              >
                {t(`${key}.save`)}
              </button>
            </SettingContainer>
          )}
        </>
      )}
    </>
  );
};

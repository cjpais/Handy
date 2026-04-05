import React, { useState, useCallback, useEffect, useMemo } from "react";
import { useTranslation } from "react-i18next";
import { useSettings } from "../../hooks/useSettings";
import { useModelStore } from "../../stores/modelStore";
import { SettingContainer } from "../ui/SettingContainer";
import { Textarea } from "../ui/Textarea";
import { Dropdown } from "../ui/Dropdown";
import type { DropdownOption } from "../ui/Dropdown";

interface TranscriptionPromptProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

function estimateTokens(text: string): number {
  let tokens = 0;
  for (const ch of text) {
    const code = ch.codePointAt(0)!;
    if (
      (code >= 0x3000 && code <= 0x9fff) ||
      (code >= 0xf900 && code <= 0xfaff) ||
      (code >= 0xff00 && code <= 0xffef)
    ) {
      tokens += 2.2; // CJK ideographs, compatibility, fullwidth
    } else if (code >= 0x0400 && code <= 0x04ff) {
      tokens += 0.5; // Cyrillic
    } else {
      tokens += 0.25; // Latin/spaces/punctuation
    }
  }
  return Math.round(tokens);
}

const TOKEN_BUDGET = 112;

const PRESETS: Record<string, string> = {
  english: `Hello! How are you? He said: "Let's do this today — while we have time." Of course, it's not that simple.`,
  spanish: `¡Hola! ¿Cómo estás? Él dijo: "Hagámoslo hoy, mientras tengamos tiempo." Claro, no es tan sencillo.`,
  french: `Bonjour ! Comment allez-vous ? Il a dit : « Faisons-le aujourd'hui — tant qu'on a le temps. » Ce n'est pas si simple.`,
  german: `Hallo! Wie geht es Ihnen? Er sagte: „Machen wir es heute — solange wir Zeit haben." So einfach ist es nicht.`,
  portuguese: `Olá! Como você está? Ele disse: "Vamos fazer isso hoje — enquanto temos tempo." Claro, não é tão simples.`,
  italian: `Ciao! Come stai? Ha detto: "Facciamolo oggi — finché abbiamo tempo." Non è così semplice.`,
  russian: `Привет! Как дела? Он сказал: «Сделаем это сегодня — пока есть время». Конечно, не всё так просто; нужно учесть погоду.`,
  japanese: `こんにちは！元気ですか？「今日やりましょう。」もちろん、簡単ではない。`,
  chinese_simplified: `你好！你怎么样？他说："今天就做吧。"当然，事情没那么简单。`,
  chinese_traditional: `你好！你怎麼樣？他說：「今天就做吧。」當然，事情沒那麼簡單。`,
};

export const TranscriptionPrompt: React.FC<TranscriptionPromptProps> =
  React.memo(({ descriptionMode = "tooltip", grouped = false }) => {
    const { t } = useTranslation();
    const { getSetting, updateSetting, isUpdating } = useSettings();
    const currentPrompt = getSetting("transcription_prompt") ?? "";
    const selectedLanguage = getSetting("selected_language");
    const currentModelId = useModelStore((s) => s.currentModel);
    const getModelInfo = useModelStore((s) => s.getModelInfo);
    const isWhisper =
      getModelInfo(currentModelId)?.engine_type === "Whisper";
    const [localValue, setLocalValue] = useState(currentPrompt);
    const [isDirty, setIsDirty] = useState(false);

    const activePreset =
      Object.entries(PRESETS).find(
        ([, text]) => text === localValue.trim(),
      )?.[0] ?? "none";

    const presetOptions: DropdownOption[] = useMemo(
      () => [
        {
          value: "none",
          label: t("settings.advanced.transcriptionPrompt.presets.none"),
        },
        {
          value: "english",
          label: t("settings.advanced.transcriptionPrompt.presets.english"),
        },
        {
          value: "spanish",
          label: t("settings.advanced.transcriptionPrompt.presets.spanish"),
        },
        {
          value: "french",
          label: t("settings.advanced.transcriptionPrompt.presets.french"),
        },
        {
          value: "german",
          label: t("settings.advanced.transcriptionPrompt.presets.german"),
        },
        {
          value: "portuguese",
          label: t("settings.advanced.transcriptionPrompt.presets.portuguese"),
        },
        {
          value: "italian",
          label: t("settings.advanced.transcriptionPrompt.presets.italian"),
        },
        {
          value: "russian",
          label: t("settings.advanced.transcriptionPrompt.presets.russian"),
        },
        {
          value: "japanese",
          label: t("settings.advanced.transcriptionPrompt.presets.japanese"),
        },
        {
          value: "chinese_simplified",
          label: t(
            "settings.advanced.transcriptionPrompt.presets.chineseSimplified",
          ),
        },
        {
          value: "chinese_traditional",
          label: t(
            "settings.advanced.transcriptionPrompt.presets.chineseTraditional",
          ),
        },
      ],
      [t],
    );

    useEffect(() => {
      if (!isDirty) {
        setLocalValue(currentPrompt);
      }
    }, [currentPrompt, isDirty]);

    const handleChange = useCallback(
      (e: React.ChangeEvent<HTMLTextAreaElement>) => {
        const value = e.target.value;
        if (estimateTokens(value) <= TOKEN_BUDGET) {
          setLocalValue(value);
          setIsDirty(true);
        }
      },
      [],
    );

    const handleBlur = useCallback(() => {
      if (!isDirty) return;
      const trimmed = localValue.trim();
      updateSetting(
        "transcription_prompt",
        trimmed.length > 0 ? trimmed : null,
      );
      setIsDirty(false);
    }, [localValue, isDirty, updateSetting]);

    const handlePreset = useCallback(
      (key: string) => {
        if (key === "none") {
          setLocalValue("");
          updateSetting("transcription_prompt", null);
        } else {
          const preset = PRESETS[key] ?? "";
          setLocalValue(preset);
          updateSetting("transcription_prompt", preset);
        }
        setIsDirty(false);
      },
      [updateSetting],
    );

    const estimatedTokens = estimateTokens(localValue);
    const percentage = Math.min(
      100,
      Math.round((estimatedTokens / TOKEN_BUDGET) * 100),
    );

    return (
      <SettingContainer
        title={t("settings.advanced.transcriptionPrompt.title")}
        description={t("settings.advanced.transcriptionPrompt.description")}
        descriptionMode={descriptionMode}
        grouped={grouped}
        layout="stacked"
      >
        <div className="flex flex-col gap-2 w-full">
          <div className="flex items-center gap-2">
            <label className="text-xs text-mid-gray">
              {t("settings.advanced.transcriptionPrompt.presets.label")}
            </label>
            <Dropdown
              options={presetOptions}
              selectedValue={activePreset}
              onSelect={handlePreset}
              disabled={isUpdating("transcription_prompt")}
              className="min-w-[140px]"
            />
          </div>
          <Textarea
            variant="compact"
            className="w-full"
            value={localValue}
            onChange={handleChange}
            onBlur={handleBlur}
            placeholder={t("settings.advanced.transcriptionPrompt.placeholder")}
            disabled={isUpdating("transcription_prompt")}
          />
          <div className="flex items-start justify-between gap-2 text-xs">
            <div className="flex flex-col gap-0.5 text-yellow-500">
              {!isWhisper && (
                <span>
                  {t("settings.advanced.transcriptionPrompt.whisperOnly")}
                </span>
              )}
              {selectedLanguage === "auto" && localValue.length > 0 && (
                <span>
                  {t("settings.advanced.transcriptionPrompt.languageWarning")}
                </span>
              )}
            </div>
            <div className="flex items-center gap-2 shrink-0">
              <div className="w-24 h-1.5 rounded-full bg-mid-gray/20 overflow-hidden">
                <div
                  className={`h-full rounded-full transition-all ${
                    percentage >= 95
                      ? "bg-red-400"
                      : percentage >= 80
                        ? "bg-yellow-400"
                        : "bg-mid-gray/50"
                  }`}
                  style={{ width: `${percentage}%` }}
                />
              </div>
              <span className="text-mid-gray text-xs tabular-nums">
                {percentage}%
              </span>
            </div>
          </div>
          {localValue.length > 0 && (
            <span className="text-mid-gray/60 text-xs">
              {t("settings.advanced.transcriptionPrompt.tokenBudgetHint")}
            </span>
          )}
        </div>
      </SettingContainer>
    );
  });

import React from "react";
import { useTranslation } from "react-i18next";
import { SettingsGroup } from "../../ui/SettingsGroup";
import { Slider } from "../../ui/Slider";
import { LanguageSelector } from "../LanguageSelector";
import { TranslateToEnglish } from "../TranslateToEnglish";
import { useModelStore } from "../../../stores/modelStore";
import { useSettings } from "../../../hooks/useSettings";
import type { ModelInfo } from "@/bindings";
import {
  CHINESE_LANGUAGE_CODE,
  getUniqueCapabilityLanguages,
} from "@/lib/constants/languages";

// Per-model latency tiers. Each array has 4 entries for slider positions 0-3 (Speed → Accuracy).
// Cache-aware models use att_context_right; Parakeet Unified uses (left_ms, chunk_ms, right_ms).
const NEMOTRON_35_TIERS = [0, 3, 6, 13] as const;
const NEMOTRON_SPEECH_TIERS = [0, 1, 6, 13] as const;
const PARAKEET_UNIFIED_TIERS = [
  { left_ms: 5600, chunk_ms: 160, right_ms: 160 },
  { left_ms: 5600, chunk_ms: 160, right_ms: 320 },
  { left_ms: 5600, chunk_ms: 560, right_ms: 560 },
  { left_ms: 5600, chunk_ms: 1040, right_ms: 1040 },
] as const;

function isNemotron35Model(modelSlug: string | undefined): boolean {
  return !!modelSlug && modelSlug.includes("nemotron-3.5-asr-streaming");
}

function isNemotronSpeechModel(modelSlug: string | undefined): boolean {
  return !!modelSlug && modelSlug.includes("nemotron-speech-streaming");
}

function isParakeetUnifiedModel(modelSlug: string | undefined): boolean {
  return !!modelSlug && modelSlug.includes("parakeet-unified");
}

function isStreamingModel(modelSlug: string | undefined): boolean {
  return (
    isNemotron35Model(modelSlug) ||
    isNemotronSpeechModel(modelSlug) ||
    isParakeetUnifiedModel(modelSlug)
  );
}

function getSliderPosition(
  modelSlug: string | undefined,
  settings: {
    parakeet_stream_att_context_right?: number;
    parakeet_stream_buf_chunk_ms?: number;
    parakeet_stream_buf_right_ms?: number;
  } | null,
): number {
  if (!settings) return 3;
  if (isParakeetUnifiedModel(modelSlug)) {
    const chunkMs = settings.parakeet_stream_buf_chunk_ms ?? 1040;
    const rightMs = settings.parakeet_stream_buf_right_ms ?? 1040;
    for (let i = 0; i < PARAKEET_UNIFIED_TIERS.length; i++) {
      if (
        PARAKEET_UNIFIED_TIERS[i].chunk_ms === chunkMs &&
        PARAKEET_UNIFIED_TIERS[i].right_ms === rightMs
      )
        return i;
    }
    return 3;
  }
  const ctx = settings.parakeet_stream_att_context_right ?? 13;
  const tiers = isNemotron35Model(modelSlug)
    ? NEMOTRON_35_TIERS
    : NEMOTRON_SPEECH_TIERS;
  for (let i = 0; i < tiers.length; i++) {
    if (tiers[i] === ctx) return i;
  }
  return 3;
}

export const ModelSettingsCard: React.FC = () => {
  const { t } = useTranslation();
  const { currentModel, models } = useModelStore();
  const { settings, updateSetting } = useSettings();

  const currentModelInfo = models.find((m: ModelInfo) => m.id === currentModel);

  const supportsLanguageSelection =
    currentModelInfo?.supports_language_selection ?? false;
  const capabilityLanguages = getUniqueCapabilityLanguages(
    currentModelInfo?.supported_languages ?? [],
  );
  const supportsChineseOnlyScriptSelection =
    capabilityLanguages.length === 1 &&
    capabilityLanguages[0] === CHINESE_LANGUAGE_CODE;
  const showLanguageSelector =
    supportsLanguageSelection || supportsChineseOnlyScriptSelection;
  const supportsTranslation = currentModelInfo?.supports_translation ?? false;
  const supportsStreamingLatency =
    currentModelInfo?.supports_streaming && isStreamingModel(currentModel);

  const hasAnySettings =
    showLanguageSelector || supportsTranslation || supportsStreamingLatency;

  // Don't render anything if no model is selected or no settings available
  if (!currentModel || !currentModelInfo || !hasAnySettings) {
    return null;
  }

  const sliderPosition = getSliderPosition(currentModel, settings);
  const isCacheAware = isNemotron35Model(currentModel) || isNemotronSpeechModel(currentModel);
  const isBuffered = isParakeetUnifiedModel(currentModel);

  const handleLatencyChange = async (position: number) => {
    if (isCacheAware) {
      const tiers = isNemotron35Model(currentModel)
        ? NEMOTRON_35_TIERS
        : NEMOTRON_SPEECH_TIERS;
      const value = tiers[position];
      if (value === settings?.parakeet_stream_att_context_right) return;
      await updateSetting("parakeet_stream_att_context_right", value);
    } else if (isBuffered) {
      const tier = PARAKEET_UNIFIED_TIERS[position];
      if (
        settings &&
        tier.left_ms === settings.parakeet_stream_buf_left_ms &&
        tier.chunk_ms === settings.parakeet_stream_buf_chunk_ms &&
        tier.right_ms === settings.parakeet_stream_buf_right_ms
      ) {
        return;
      }
      await updateSetting("parakeet_stream_buf_left_ms", tier.left_ms);
      await updateSetting("parakeet_stream_buf_chunk_ms", tier.chunk_ms);
      await updateSetting("parakeet_stream_buf_right_ms", tier.right_ms);
    }
  };

  const handleLatencyReset = async () => {
    if (isCacheAware) {
      await updateSetting("parakeet_stream_att_context_right", 13);
    } else if (isBuffered) {
      await updateSetting("parakeet_stream_buf_left_ms", 5600);
      await updateSetting("parakeet_stream_buf_chunk_ms", 1040);
      await updateSetting("parakeet_stream_buf_right_ms", 1040);
    }
  };

  return (
    <SettingsGroup
      title={t("settings.modelSettings.title", {
        model: currentModelInfo.name,
      })}
    >
      {showLanguageSelector && (
        <LanguageSelector
          descriptionMode="tooltip"
          grouped={true}
          supportedLanguages={currentModelInfo.supported_languages}
          supportsLanguageDetection={
            currentModelInfo.supports_language_detection
          }
        />
      )}
      {supportsTranslation && (
        <TranslateToEnglish descriptionMode="tooltip" grouped={true} />
      )}
      {supportsStreamingLatency && (
        <Slider
          label={t("settings.modelSettings.streamingLatency.label")}
          description={t("settings.modelSettings.streamingLatency.description")}
          descriptionMode="tooltip"
          grouped={true}
          min={0}
          max={3}
          step={1}
          value={sliderPosition}
          onChange={handleLatencyChange}
          disabled={!settings}
          showValue={false}
          resetAction={handleLatencyReset}
        />
      )}
    </SettingsGroup>
  );
};

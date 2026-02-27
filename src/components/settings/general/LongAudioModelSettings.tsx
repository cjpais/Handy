import React from "react";
import { useTranslation } from "react-i18next";
import { SettingsGroup } from "../../ui/SettingsGroup";
import { SettingContainer } from "../../ui/SettingContainer";
import { Dropdown } from "../../ui/Dropdown";
import { useSettings } from "../../../hooks/useSettings";
import { useModelStore } from "../../../stores/modelStore";
import type { ModelInfo } from "@/bindings";

const THRESHOLD_OPTIONS = [5, 10, 15, 20, 30, 60];

export const LongAudioModelSettings: React.FC = () => {
  const { t } = useTranslation();
  const { getSetting, updateSetting } = useSettings();
  const { models } = useModelStore();

  const downloadedModels = models.filter((m: ModelInfo) => m.is_downloaded);
  const longAudioModel = getSetting("long_audio_model") ?? null;
  const threshold = getSetting("long_audio_threshold_seconds") ?? 10;

  const modelOptions = [
    { value: "", label: t("settings.longAudioModel.disabled") },
    ...downloadedModels.map((m: ModelInfo) => ({
      value: m.id,
      label: m.name,
    })),
  ];

  const thresholdOptions = THRESHOLD_OPTIONS.map((v) => ({
    value: String(v),
    label: t("settings.longAudioModel.seconds", { value: v }),
  }));

  const handleModelSelect = async (value: string) => {
    await updateSetting(
      "long_audio_model",
      value === "" ? null : value,
    );
  };

  const handleThresholdSelect = async (value: string) => {
    await updateSetting("long_audio_threshold_seconds", Number(value));
  };

  return (
    <SettingsGroup
      title={t("settings.longAudioModel.title")}
      description={t("settings.longAudioModel.description")}
    >
      <SettingContainer
        title={t("settings.longAudioModel.modelLabel")}
        description={t("settings.longAudioModel.modelDescription")}
        descriptionMode="tooltip"
        grouped={true}
      >
        <Dropdown
          options={modelOptions}
          selectedValue={longAudioModel ?? ""}
          onSelect={handleModelSelect}
        />
      </SettingContainer>
      {longAudioModel && (
        <SettingContainer
          title={t("settings.longAudioModel.thresholdLabel")}
          description={t("settings.longAudioModel.thresholdDescription")}
          descriptionMode="tooltip"
          grouped={true}
        >
          <Dropdown
            options={thresholdOptions}
            selectedValue={String(threshold)}
            onSelect={handleThresholdSelect}
          />
        </SettingContainer>
      )}
    </SettingsGroup>
  );
};

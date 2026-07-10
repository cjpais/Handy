import React from "react";
import { useTranslation } from "react-i18next";
import { open } from "@tauri-apps/plugin-dialog";
import { toast } from "sonner";
import { SettingsGroup } from "../ui/SettingsGroup";
import { SettingContainer } from "../ui/SettingContainer";
import { ToggleSwitch } from "../ui/ToggleSwitch";
import { Dropdown, type DropdownOption } from "../ui/Dropdown";
import { Slider } from "../ui/Slider";
import { Button } from "../ui/Button";
import { useSettings } from "../../hooks/useSettings";
import { commands, type WakeWordModel } from "@/bindings";

const MODEL_OPTIONS: WakeWordModel[] = [
  "jarvis",
  "hey_jarvis",
  "alexa",
  "hey_mycroft",
  "hey_rhasspy",
  "custom",
];

export const WakeWordSettings: React.FC = () => {
  const { t } = useTranslation();
  const { getSetting, updateSetting, isUpdating, refreshSettings } =
    useSettings();

  const enabled = getSetting("wake_word_enabled") || false;
  const model = (getSetting("wake_word_model") ??
    "hey_jarvis") as WakeWordModel;
  const customPath = getSetting("wake_word_custom_model_path") ?? null;
  const threshold = getSetting("wake_word_threshold") ?? 0.5;
  const silenceTimeoutMs = getSetting("wake_word_silence_timeout_ms") ?? 2000;

  const modelOptions: DropdownOption[] = MODEL_OPTIONS.map((value) => ({
    value,
    label: t(`settings.wakeWord.model.options.${value}`),
  }));

  const pickCustomModel = async () => {
    const selected = await open({
      multiple: false,
      filters: [{ name: "ONNX model", extensions: ["onnx"] }],
    });
    if (typeof selected !== "string") return;
    // Called directly (not through updateSetting) so the backend's model
    // validation error can be surfaced as a toast.
    const result =
      await commands.changeWakeWordCustomModelPathSetting(selected);
    if (result.status === "error") {
      toast.error(
        t("settings.wakeWord.customModel.invalid", { error: result.error }),
      );
      return;
    }
    await refreshSettings();
  };

  return (
    <SettingsGroup title={t("settings.wakeWord.title")}>
      <ToggleSwitch
        checked={enabled}
        onChange={(value) => updateSetting("wake_word_enabled", value)}
        isUpdating={isUpdating("wake_word_enabled")}
        label={t("settings.wakeWord.enabled.label")}
        description={t("settings.wakeWord.enabled.description")}
        descriptionMode="tooltip"
        grouped
      />
      <SettingContainer
        title={t("settings.wakeWord.model.title")}
        description={t("settings.wakeWord.model.description")}
        descriptionMode="tooltip"
        grouped
        layout="horizontal"
      >
        <Dropdown
          options={modelOptions}
          selectedValue={model}
          onSelect={(value) =>
            updateSetting("wake_word_model", value as WakeWordModel)
          }
          disabled={!enabled || isUpdating("wake_word_model")}
        />
      </SettingContainer>
      {model === "custom" && (
        <SettingContainer
          title={t("settings.wakeWord.customModel.title")}
          description={t("settings.wakeWord.customModel.description")}
          descriptionMode="tooltip"
          grouped
          layout="horizontal"
        >
          <div className="flex items-center gap-2 min-w-0">
            {customPath && (
              <span
                className="text-xs text-muted-foreground truncate max-w-48"
                dir="rtl"
                title={customPath}
              >
                {customPath}
              </span>
            )}
            <Button
              variant="secondary"
              size="sm"
              onClick={pickCustomModel}
              disabled={!enabled || isUpdating("wake_word_custom_model_path")}
            >
              {t("settings.wakeWord.customModel.choose")}
            </Button>
          </div>
        </SettingContainer>
      )}
      <Slider
        value={threshold}
        onChange={(value) => updateSetting("wake_word_threshold", value)}
        min={0.1}
        max={0.9}
        step={0.05}
        label={t("settings.wakeWord.threshold.title")}
        description={t("settings.wakeWord.threshold.description")}
        descriptionMode="tooltip"
        grouped
        formatValue={(value) => value.toFixed(2)}
        disabled={!enabled}
      />
      <Slider
        value={silenceTimeoutMs}
        onChange={(value) =>
          updateSetting("wake_word_silence_timeout_ms", value)
        }
        min={500}
        max={5000}
        step={250}
        label={t("settings.wakeWord.silenceTimeout.title")}
        description={t("settings.wakeWord.silenceTimeout.description")}
        descriptionMode="tooltip"
        grouped
        formatValue={(value) => `${(value / 1000).toFixed(2)} s`}
        disabled={!enabled}
      />
    </SettingsGroup>
  );
};

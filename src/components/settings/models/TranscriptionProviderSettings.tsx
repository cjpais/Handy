import React from "react";
import { useTranslation } from "react-i18next";
import type { TranscriptionProvider } from "@/bindings";
import { useSettings } from "@/hooks/useSettings";
import { Dropdown } from "@/components/ui/Dropdown";
import { Input } from "@/components/ui/Input";
import { SettingContainer } from "@/components/ui/SettingContainer";
import { SettingsGroup } from "@/components/ui/SettingsGroup";
import { ToggleSwitch } from "@/components/ui/ToggleSwitch";

export const TranscriptionProviderSettings: React.FC = () => {
  const { t } = useTranslation();
  const { settings, updateSetting, isUpdating } = useSettings();

  const provider = settings?.transcription_provider ?? "local";
  const isSoniox = provider === "soniox";
  const isSlng = provider === "slng";

  const providerOptions = [
    {
      value: "local",
      label: t("settings.models.provider.options.local"),
    },
    {
      value: "soniox",
      label: t("settings.models.provider.options.soniox"),
    },
    {
      value: "slng",
      label: t("settings.models.provider.options.slng"),
    },
  ];

  const updateSonioxTimeout = (value: string) => {
    const timeout = Number.parseInt(value, 10);
    if (Number.isNaN(timeout)) {
      return;
    }
    updateSetting("soniox_timeout_seconds", Math.max(1, timeout));
  };

  const updateSlngTimeout = (value: string) => {
    const timeout = Number.parseInt(value, 10);
    if (Number.isNaN(timeout)) {
      return;
    }
    updateSetting("slng_timeout_seconds", Math.max(1, timeout));
  };

  return (
    <SettingsGroup title={t("settings.models.provider.title")}>
      <SettingContainer
        title={t("settings.models.provider.selector.title")}
        description={t("settings.models.provider.selector.description")}
        grouped={true}
      >
        <Dropdown
          options={providerOptions}
          selectedValue={provider}
          onSelect={(value) =>
            updateSetting(
              "transcription_provider",
              value as TranscriptionProvider,
            )
          }
          disabled={isUpdating("transcription_provider")}
        />
      </SettingContainer>

      {isSoniox && (
        <>
          <SettingContainer
            title={t("settings.models.provider.soniox.apiKey.title")}
            description={t(
              "settings.models.provider.soniox.apiKey.description",
            )}
            grouped={true}
            layout="stacked"
          >
            <Input
              type="password"
              value={settings?.soniox_api_key ?? ""}
              onChange={(event) =>
                updateSetting("soniox_api_key", event.target.value)
              }
              placeholder={t(
                "settings.models.provider.soniox.apiKey.placeholder",
              )}
              disabled={isUpdating("soniox_api_key")}
              className="w-full"
            />
          </SettingContainer>

          <SettingContainer
            title={t("settings.models.provider.soniox.model.title")}
            description={t("settings.models.provider.soniox.model.description")}
            grouped={true}
          >
            <Input
              value={settings?.soniox_model ?? ""}
              onChange={(event) =>
                updateSetting("soniox_model", event.target.value)
              }
              placeholder={t(
                "settings.models.provider.soniox.model.placeholder",
              )}
              disabled={isUpdating("soniox_model")}
              className="min-w-[220px]"
            />
          </SettingContainer>

          <SettingContainer
            title={t("settings.models.provider.soniox.timeout.title")}
            description={t(
              "settings.models.provider.soniox.timeout.description",
            )}
            grouped={true}
          >
            <Input
              type="number"
              min={1}
              value={settings?.soniox_timeout_seconds ?? 120}
              onChange={(event) => updateSonioxTimeout(event.target.value)}
              disabled={isUpdating("soniox_timeout_seconds")}
              className="w-28"
            />
          </SettingContainer>

          <ToggleSwitch
            checked={settings?.soniox_fallback_to_local ?? true}
            onChange={(checked) =>
              updateSetting("soniox_fallback_to_local", checked)
            }
            isUpdating={isUpdating("soniox_fallback_to_local")}
            label={t("settings.models.provider.soniox.fallback.label")}
            description={t(
              "settings.models.provider.soniox.fallback.description",
            )}
            grouped={true}
          />
        </>
      )}

      {isSlng && (
        <>
          <SettingContainer
            title={t("settings.models.provider.slng.apiKey.title")}
            description={t("settings.models.provider.slng.apiKey.description")}
            grouped={true}
            layout="stacked"
          >
            <Input
              type="password"
              value={settings?.slng_api_key ?? ""}
              onChange={(event) =>
                updateSetting("slng_api_key", event.target.value)
              }
              placeholder={t(
                "settings.models.provider.slng.apiKey.placeholder",
              )}
              disabled={isUpdating("slng_api_key")}
              className="w-full"
            />
          </SettingContainer>

          <SettingContainer
            title={t("settings.models.provider.slng.provider.title")}
            description={t(
              "settings.models.provider.slng.provider.description",
            )}
            grouped={true}
          >
            <Input
              value={settings?.slng_provider ?? ""}
              onChange={(event) =>
                updateSetting("slng_provider", event.target.value)
              }
              placeholder={t(
                "settings.models.provider.slng.provider.placeholder",
              )}
              disabled={isUpdating("slng_provider")}
              className="min-w-[220px]"
            />
          </SettingContainer>

          <SettingContainer
            title={t("settings.models.provider.slng.model.title")}
            description={t("settings.models.provider.slng.model.description")}
            grouped={true}
          >
            <Input
              value={settings?.slng_model ?? ""}
              onChange={(event) =>
                updateSetting("slng_model", event.target.value)
              }
              placeholder={t("settings.models.provider.slng.model.placeholder")}
              disabled={isUpdating("slng_model")}
              className="min-w-[220px]"
            />
          </SettingContainer>

          <SettingContainer
            title={t("settings.models.provider.slng.timeout.title")}
            description={t("settings.models.provider.slng.timeout.description")}
            grouped={true}
          >
            <Input
              type="number"
              min={1}
              value={settings?.slng_timeout_seconds ?? 120}
              onChange={(event) => updateSlngTimeout(event.target.value)}
              disabled={isUpdating("slng_timeout_seconds")}
              className="w-28"
            />
          </SettingContainer>

          <ToggleSwitch
            checked={settings?.slng_fallback_to_local ?? true}
            onChange={(checked) =>
              updateSetting("slng_fallback_to_local", checked)
            }
            isUpdating={isUpdating("slng_fallback_to_local")}
            label={t("settings.models.provider.slng.fallback.label")}
            description={t(
              "settings.models.provider.slng.fallback.description",
            )}
            grouped={true}
          />
        </>
      )}
    </SettingsGroup>
  );
};

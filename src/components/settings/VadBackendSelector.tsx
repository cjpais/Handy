import React, { useMemo } from "react";
import { useTranslation } from "react-i18next";
import type { VadBackend } from "@/bindings";
import { useSettings } from "@/hooks/useSettings";
import { Dropdown, type DropdownOption } from "@/components/ui/Dropdown";
import { SettingContainer } from "@/components/ui/SettingContainer";

interface VadBackendSelectorProps {
  descriptionMode?: "tooltip" | "inline";
  grouped?: boolean;
}

export const VadBackendSelector: React.FC<VadBackendSelectorProps> = ({
  descriptionMode = "tooltip",
  grouped = false,
}) => {
  const { t } = useTranslation();
  const { getSetting, updateSetting, isUpdating } = useSettings();
  const selectedBackend = getSetting("vad_backend") ?? "silero";

  const options = useMemo<DropdownOption[]>(
    () => [
      {
        value: "silero",
        label: t("settings.advanced.vadBackend.options.silero"),
      },
      {
        value: "earshot",
        label: t("settings.advanced.vadBackend.options.earshot"),
      },
    ],
    [t],
  );

  return (
    <SettingContainer
      title={t("settings.advanced.vadBackend.title")}
      description={t("settings.advanced.vadBackend.description")}
      descriptionMode={descriptionMode}
      grouped={grouped}
      layout="horizontal"
    >
      <Dropdown
        options={options}
        selectedValue={selectedBackend}
        onSelect={(value) => updateSetting("vad_backend", value as VadBackend)}
        disabled={isUpdating("vad_backend")}
      />
    </SettingContainer>
  );
};

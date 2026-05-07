import React, { useMemo } from "react";
import { useTranslation } from "react-i18next";
import type { TranscriptionBackend } from "@/bindings";
import { Select } from "../../ui/Select";
import { SettingContainer } from "../../ui/SettingContainer";

interface BackendSelectorProps {
  backend: TranscriptionBackend;
  disabled: boolean;
  onChange: (value: TranscriptionBackend) => Promise<void>;
}

export const BackendSelector: React.FC<BackendSelectorProps> = ({
  backend,
  disabled,
  onChange,
}) => {
  const { t } = useTranslation();

  const options = useMemo(
    () => [
      {
        value: "local",
        label: t("settings.transcriptionBackend.options.local"),
      },
      {
        value: "live_stt",
        label: t("settings.transcriptionBackend.options.livestt"),
      },
    ],
    [t],
  );

  const handleChange = async (value: string | null) => {
    if (!value) {
      return;
    }

    await onChange(value as TranscriptionBackend);
  };

  return (
    <SettingContainer
      title={t("settings.transcriptionBackend.selector.title")}
      description={t("settings.transcriptionBackend.selector.description")}
      descriptionMode="tooltip"
      grouped={true}
    >
      <Select
        value={backend}
        options={options}
        onChange={handleChange}
        disabled={disabled}
        isClearable={false}
        className="min-w-[220px]"
      />
    </SettingContainer>
  );
};

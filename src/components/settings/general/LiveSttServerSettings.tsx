import React from "react";
import { useTranslation } from "react-i18next";
import { Input } from "../../ui/Input";
import { SettingContainer } from "../../ui/SettingContainer";

interface LiveSttServerSettingsProps {
  value: string;
  error: string | null;
  disabled: boolean;
  onChange: (value: string) => void;
  onBlur: () => Promise<void>;
}

export const LiveSttServerSettings: React.FC<LiveSttServerSettingsProps> = ({
  value,
  error,
  disabled,
  onChange,
  onBlur,
}) => {
  const { t } = useTranslation();

  return (
    <SettingContainer
      title={t("settings.transcriptionBackend.livestt.serverUrl.title")}
      description={t(
        "settings.transcriptionBackend.livestt.serverUrl.description",
      )}
      descriptionMode="tooltip"
      grouped={true}
      layout="stacked"
    >
      <Input
        type="url"
        value={value}
        onChange={(event) => onChange(event.target.value)}
        onBlur={() => {
          void onBlur();
        }}
        placeholder={t(
          "settings.transcriptionBackend.livestt.serverUrl.placeholder",
        )}
        disabled={disabled}
        className="w-full"
      />
      {error && <p className="mt-2 text-xs text-red-500">{error}</p>}
    </SettingContainer>
  );
};

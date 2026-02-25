import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Input } from "../ui/Input";
import { SettingContainer } from "../ui/SettingContainer";
import { useSettings } from "../../hooks/useSettings";

interface LocalApiTokenProps {
  descriptionMode?: "tooltip" | "inline";
  grouped?: boolean;
}

export const LocalApiToken: React.FC<LocalApiTokenProps> = ({
  descriptionMode = "inline",
  grouped = false,
}) => {
  const { t } = useTranslation();
  const { getSetting, updateSetting, isUpdating } = useSettings();

  const enabled = getSetting("local_api_enabled") ?? false;
  const storedToken = getSetting("local_api_token") ?? "";
  const [value, setValue] = useState(storedToken);

  useEffect(() => {
    setValue(storedToken);
  }, [storedToken]);

  return (
    <SettingContainer
      title={t("settings.advanced.localApiToken.title")}
      description={t("settings.advanced.localApiToken.description")}
      descriptionMode={descriptionMode}
      grouped={grouped}
      disabled={!enabled}
    >
      <Input
        type="password"
        value={value}
        onChange={(event) => setValue(event.target.value)}
        onBlur={() => updateSetting("local_api_token", value.trim() || null)}
        placeholder={t("settings.advanced.localApiToken.placeholder")}
        variant="compact"
        disabled={!enabled || isUpdating("local_api_token")}
        className="flex-1 min-w-[320px]"
      />
    </SettingContainer>
  );
};

import React, { useState } from "react";
import { useTranslation } from "react-i18next";
import { useSettings } from "../../../hooks/useSettings";
import { SettingContainer } from "../../ui/SettingContainer";
import { Dropdown } from "../../ui/Dropdown";
import { Input } from "../../ui/Input";

const GEMINI_MODELS = [
  { value: "gemini-2.5-flash", label: "Gemini 2.5 Flash" },
  { value: "gemini-2.5-pro", label: "Gemini 2.5 Pro" },
  { value: "gemini-3-flash-preview", label: "Gemini 3 Flash" },
];

export const GeminiSettings: React.FC = () => {
  const { t } = useTranslation();
  const { getSetting, updateSetting } = useSettings();

  const apiKey = (getSetting("gemini_api_key") as string | undefined) ?? "";
  const currentModel =
    (getSetting("gemini_model") as string | undefined) ?? "gemini-2.5-flash";
  const [localApiKey, setLocalApiKey] = useState(apiKey);

  React.useEffect(() => {
    setLocalApiKey(apiKey);
  }, [apiKey]);

  const handleApiKeyBlur = () => {
    if (localApiKey !== apiKey) {
      updateSetting("gemini_api_key", localApiKey || null);
    }
  };

  return (
    <>
      <SettingContainer
        title={t("settings.gemini.apiKey")}
        description={t("settings.gemini.description")}
        descriptionMode="tooltip"
        layout="horizontal"
        grouped={true}
      >
        <div className="flex items-center justify-end gap-2">
          <Input
            type="password"
            value={localApiKey}
            onChange={(e) => setLocalApiKey(e.target.value)}
            onBlur={handleApiKeyBlur}
            placeholder={t("settings.gemini.apiKeyPlaceholder")}
            variant="compact"
            className="flex-1 w-[280px]"
          />
        </div>
      </SettingContainer>

      <SettingContainer
        title={t("settings.gemini.model")}
        description={t("settings.gemini.modelDescription")}
        descriptionMode="tooltip"
        layout="horizontal"
        grouped={true}
      >
        <div className="flex items-center justify-end gap-2">
          <Dropdown
            options={GEMINI_MODELS}
            selectedValue={currentModel}
            onSelect={(value) => updateSetting("gemini_model", value)}
            className="w-[280px]"
          />
        </div>
      </SettingContainer>
    </>
  );
};

import React, { useState, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { Check } from "lucide-react";

import { SettingsGroup } from "../../ui/SettingsGroup";
import { SettingContainer } from "../../ui/SettingContainer";
import { Input } from "../../ui/Input";
import { Dropdown } from "../../ui/Dropdown";
import { Textarea } from "../../ui/Textarea";
import { Button } from "../../ui/Button";
import { useSettings } from "../../../hooks/useSettings";

const ONLINE_PROVIDERS = [
    { value: "openai", label: "OpenAI" },
    { value: "groq", label: "Groq" },
    { value: "gemini", label: "Gemini" },
    { value: "sambanova", label: "SambaNova" },
];

const PROVIDER_MODELS: Record<string, { value: string; label: string }[]> = {
    openai: [
        { value: "whisper", label: "Whisper" },
    ],
    groq: [
        { value: "whisper-large-v3", label: "Whisper Large V3" },
        { value: "whisper-large-v3-turbo", label: "Whisper Large V3 Turbo" },
    ],
    gemini: [
        { value: "gemini-2.5-flash", label: "Gemini 2.5 Flash" },
        { value: "gemini-2.5-flash-lite", label: "Gemini 2.5 Flash Lite" },
        { value: "gemini-2.5-pro", label: "Gemini 2.5 Pro" },
        { value: "gemini-2.0-flash", label: "Gemini 2.0 Flash" },
        { value: "gemini-2.0-flash-lite", label: "Gemini 2.0 Flash Lite" },
        { value: "other", label: "Other (Custom)" },
    ],
    sambanova: [
        { value: "whisper-large-v3", label: "Whisper Large V3" },
    ],
};

const DisabledNotice: React.FC<{ children: React.ReactNode }> = ({
    children,
}) => (
    <div className="p-4 bg-mid-gray/5 rounded-lg border border-mid-gray/20">
        <p className="text-sm text-mid-gray">{children}</p>
    </div>
);

export const OnlineProviderSettings: React.FC = () => {
    const { t } = useTranslation();
    const { getSetting, updateSetting, isUpdating } = useSettings();

    const enabled = getSetting("use_online_provider") || false;
    const selectedProviderId = getSetting("online_provider_id") || "openai";
    const apiKeys = getSetting("online_provider_api_keys") || {};
    const customPrompt = getSetting("online_provider_custom_prompt") || "";

    const [localApiKey, setLocalApiKey] = useState(apiKeys[selectedProviderId] || "");
    const [localCustomPrompt, setLocalCustomPrompt] = useState(customPrompt || "");
    const [selectedModel, setSelectedModel] = useState<string>("");
    const [customModelId, setCustomModelId] = useState("");

    const modelOptions = PROVIDER_MODELS[selectedProviderId] || [];
    const isOtherSelected = selectedModel === "other";

    // Set default model when provider changes
    useEffect(() => {
        if (modelOptions.length > 0) {
            setSelectedModel(modelOptions[0].value);
            setCustomModelId("");
        }
    }, [selectedProviderId, modelOptions]);

    const isApiKeyDirty = localApiKey !== (apiKeys[selectedProviderId] || "");
    const isApiKeySaving = isUpdating("online_provider_api_keys");

    const handleProviderChange = (providerId: string | null) => {
        if (!providerId) return;
        updateSetting("online_provider_id", providerId);
        // Update local API key to show the key for the new provider
        setLocalApiKey(apiKeys[providerId] || "");
    };

    const handleModelChange = (modelId: string | null) => {
        if (!modelId) return;
        setSelectedModel(modelId);
        if (modelId !== "other") {
            setCustomModelId("");
        }
    };

    const handleSaveApiKey = () => {
        if (isApiKeyDirty) {
            const updatedKeys = { ...apiKeys, [selectedProviderId]: localApiKey };
            updateSetting("online_provider_api_keys", updatedKeys);
        }
    };

    const handleCustomPromptBlur = () => {
        if (localCustomPrompt !== (customPrompt || "")) {
            updateSetting("online_provider_custom_prompt", localCustomPrompt || null);
        }
    };

    if (!enabled) {
        return (
            <div className="max-w-3xl w-full mx-auto space-y-6">
                <SettingsGroup title={t("settings.onlineProviders.title")}>
                    <DisabledNotice>
                        {t("settings.onlineProviders.disabledNotice")}
                    </DisabledNotice>
                </SettingsGroup>
            </div>
        );
    }

    return (
        <div className="max-w-3xl w-full mx-auto space-y-6">
            <SettingsGroup title={t("settings.onlineProviders.title")}>
                <SettingContainer
                    title={t("settings.onlineProviders.provider.title")}
                    description={t("settings.onlineProviders.provider.description")}
                    descriptionMode="tooltip"
                    layout="horizontal"
                    grouped={true}
                >
                    <div className="flex items-center gap-2 ml-auto">
                        <Dropdown
                            selectedValue={selectedProviderId}
                            options={ONLINE_PROVIDERS}
                            onSelect={handleProviderChange}
                            disabled={isUpdating("online_provider_id")}
                            className="min-w-[200px]"
                        />
                    </div>
                </SettingContainer>

                <SettingContainer
                    title={t("settings.onlineProviders.model.title")}
                    description={t("settings.onlineProviders.model.description")}
                    descriptionMode="tooltip"
                    layout="horizontal"
                    grouped={true}
                    tooltipPosition="bottom"
                >
                    <div className="flex items-center gap-2 ml-auto">
                        <Dropdown
                            selectedValue={selectedModel}
                            options={modelOptions}
                            onSelect={handleModelChange}
                            placeholder={t("settings.onlineProviders.model.placeholder")}
                            className="min-w-[200px]"
                        />
                        {isOtherSelected && (
                            <Input
                                type="text"
                                value={customModelId}
                                onChange={(e) => setCustomModelId(e.target.value)}
                                placeholder="Enter model ID"
                                className="min-w-[180px]"
                                variant="compact"
                            />
                        )}
                    </div>
                </SettingContainer>

                <SettingContainer
                    title={t("settings.onlineProviders.apiKey.title")}
                    description={t("settings.onlineProviders.apiKey.description")}
                    descriptionMode="tooltip"
                    layout="horizontal"
                    grouped={true}
                    tooltipPosition="bottom"
                >
                    <div className="flex items-center gap-2 ml-auto">
                        <Input
                            type="password"
                            value={localApiKey}
                            onChange={(e) => setLocalApiKey(e.target.value)}
                            placeholder={t("settings.onlineProviders.apiKey.placeholder")}
                            className="min-w-[280px]"
                            variant="compact"
                        />
                        {isApiKeyDirty && (
                            <Button
                                onClick={handleSaveApiKey}
                                variant="primary"
                                size="sm"
                                disabled={isApiKeySaving}
                                className="flex items-center gap-1 px-3"
                            >
                                <Check className="h-4 w-4" />
                            </Button>
                        )}
                    </div>
                </SettingContainer>

                <SettingContainer
                    title={t("settings.onlineProviders.customPrompt.title")}
                    description={t("settings.onlineProviders.customPrompt.description")}
                    descriptionMode="tooltip"
                    layout="horizontal"
                    grouped={true}
                    tooltipPosition="bottom"
                >
                    <div className="flex items-center gap-2 ml-auto flex-1">
                        <Textarea
                            value={localCustomPrompt}
                            onChange={(e) => setLocalCustomPrompt(e.target.value)}
                            placeholder={t("settings.onlineProviders.customPrompt.placeholder")}
                            className="min-w-[250px] max-h-20"
                        />
                        {localCustomPrompt !== (customPrompt || "") && (
                            <Button
                                onClick={handleCustomPromptBlur}
                                variant="primary"
                                size="sm"
                                disabled={isUpdating("online_provider_custom_prompt")}
                                className="flex items-center gap-1 px-3"
                            >
                                <Check className="h-4 w-4" />
                            </Button>
                        )}
                    </div>
                </SettingContainer>
            </SettingsGroup>
        </div>
    );
};

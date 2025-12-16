import React, { useState, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { Check, Eye, EyeOff } from "lucide-react";

import { SettingsGroup } from "../../ui/SettingsGroup";
import { SettingContainer } from "../../ui/SettingContainer";
import { Input } from "../../ui/Input";
import { Dropdown } from "../../ui/Dropdown";
import { Button } from "../../ui/Button";
import { useSettings } from "../../../hooks/useSettings";
import { commands } from "@/bindings";

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
    const { getSetting, updateSetting, isUpdating, refreshSettings } = useSettings();

    const enabled = getSetting("use_online_provider") || false;
    const selectedProviderId = getSetting("online_provider_id") || "openai";
    const apiKeys = getSetting("online_provider_api_keys") || {};
    const savedModels = getSetting("online_provider_models") || {};

    const [localApiKey, setLocalApiKey] = useState(apiKeys[selectedProviderId] || "");
    const [showApiKey, setShowApiKey] = useState(false);
    const [selectedModel, setSelectedModel] = useState<string>(savedModels[selectedProviderId] || "");
    const [customModelId, setCustomModelId] = useState("");
    const [isApiKeySaving, setIsApiKeySaving] = useState(false);

    const modelOptions = PROVIDER_MODELS[selectedProviderId] || [];
    const isOtherSelected = selectedModel === "other";

    // Update local state when provider changes
    useEffect(() => {
        setLocalApiKey(apiKeys[selectedProviderId] || "");
        const savedModel = savedModels[selectedProviderId] || "";
        if (savedModel) {
            setSelectedModel(savedModel);
        } else if (modelOptions.length > 0) {
            setSelectedModel(modelOptions[0].value);
        }
        setCustomModelId("");
    }, [selectedProviderId, modelOptions.length]);

    const isApiKeyDirty = localApiKey !== (apiKeys[selectedProviderId] || "");

    const handleProviderChange = (providerId: string | null) => {
        if (!providerId) return;
        updateSetting("online_provider_id", providerId);
    };

    const handleModelChange = async (modelId: string | null) => {
        if (!modelId) return;
        setSelectedModel(modelId);
        if (modelId !== "other") {
            setCustomModelId("");
            // Save the model selection to backend
            try {
                await commands.changeOnlineProviderModelSetting(selectedProviderId, modelId);
                await refreshSettings();
            } catch (error) {
                console.error("Failed to save model selection:", error);
            }
        }
    };

    const handleSaveApiKey = async () => {
        if (isApiKeyDirty) {
            setIsApiKeySaving(true);
            try {
                await commands.changeOnlineProviderApiKeySetting(selectedProviderId, localApiKey);
                await refreshSettings();
            } catch (error) {
                console.error("Failed to save API key:", error);
            } finally {
                setIsApiKeySaving(false);
            }
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
                        <div className="relative flex items-center">
                            <Input
                                type={showApiKey ? "text" : "password"}
                                value={localApiKey}
                                onChange={(e) => setLocalApiKey(e.target.value)}
                                placeholder={t("settings.onlineProviders.apiKey.placeholder")}
                                className="min-w-[280px] pr-10"
                                variant="compact"
                            />
                            <button
                                type="button"
                                onClick={() => setShowApiKey(!showApiKey)}
                                className="absolute right-2 p-1 text-text/50 hover:text-logo-primary transition-colors"
                                title={showApiKey ? "Hide API key" : "Show API key"}
                            >
                                {showApiKey ? (
                                    <EyeOff className="w-4 h-4" />
                                ) : (
                                    <Eye className="w-4 h-4" />
                                )}
                            </button>
                        </div>
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

            </SettingsGroup>
        </div>
    );
};

import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { RefreshCcw } from "lucide-react";
import { commands } from "@/bindings";

import { Alert } from "../../ui/Alert";
import {
  Dropdown,
  SettingContainer,
  SettingsGroup,
  Textarea,
} from "@/components/ui";
import { Button } from "../../ui/Button";
import { ResetButton } from "../../ui/ResetButton";
import { Input } from "../../ui/Input";

import { ProviderSelect } from "../PostProcessingSettingsApi/ProviderSelect";
import { BaseUrlField } from "../PostProcessingSettingsApi/BaseUrlField";
import { ApiKeyField } from "../PostProcessingSettingsApi/ApiKeyField";
import { ModelSelect } from "../PostProcessingSettingsApi/ModelSelect";
import { usePostProcessProviderState } from "../PostProcessingSettingsApi/usePostProcessProviderState";
import { ShortcutInput } from "../ShortcutInput";
import { useSettings } from "../../../hooks/useSettings";

const PostProcessingSettingsApiComponent: React.FC = () => {
  const { t } = useTranslation();
  const state = usePostProcessProviderState();

  return (
    <>
      <SettingContainer
        title={t("settings.postProcessing.api.provider.title")}
        description={t("settings.postProcessing.api.provider.description")}
        descriptionMode="tooltip"
        layout="horizontal"
        grouped={true}
      >
        <div className="flex items-center gap-2">
          <ProviderSelect
            options={state.providerOptions}
            value={state.selectedProviderId}
            onChange={state.handleProviderSelect}
          />
        </div>
      </SettingContainer>

      {state.isAppleProvider ? (
        state.appleIntelligenceUnavailable ? (
          <Alert variant="error" contained>
            {t("settings.postProcessing.api.appleIntelligence.unavailable")}
          </Alert>
        ) : null
      ) : state.isBedrockProvider ? (
        <>
          <SettingContainer
            title={t("settings.postProcessing.api.bedrock.authType.title")}
            description={t(
              "settings.postProcessing.api.bedrock.authType.description",
            )}
            descriptionMode="tooltip"
            layout="horizontal"
            grouped={true}
          >
            <Dropdown
              options={[
                {
                  value: "credentials",
                  label: t(
                    "settings.postProcessing.api.bedrock.authType.credentials",
                  ),
                },
                {
                  value: "profile",
                  label: t(
                    "settings.postProcessing.api.bedrock.authType.profile",
                  ),
                },
              ]}
              selectedValue={
                state.bedrockUseProfile ? "profile" : "credentials"
              }
              onSelect={(v) =>
                state.handleBedrockSettingChange(
                  "use_profile",
                  v === "profile" ? "true" : "false",
                )
              }
              className="flex-1"
            />
          </SettingContainer>

          {state.bedrockUseProfile ? (
            <SettingContainer
              title={t("settings.postProcessing.api.bedrock.profile.title")}
              description={t(
                "settings.postProcessing.api.bedrock.profile.description",
              )}
              descriptionMode="tooltip"
              layout="horizontal"
              grouped={true}
            >
              <Input
                type="text"
                defaultValue={state.bedrockProfile}
                onBlur={(e) =>
                  state.handleBedrockSettingChange(
                    "profile",
                    e.target.value.trim(),
                  )
                }
                autoCapitalize="off"
                autoCorrect="off"
                autoComplete="off"
                spellCheck={false}
                placeholder={t(
                  "settings.postProcessing.api.bedrock.profile.placeholder",
                )}
                variant="compact"
                className="min-w-[200px]"
              />
            </SettingContainer>
          ) : (
            <>
              <SettingContainer
                title={t(
                  "settings.postProcessing.api.bedrock.accessKeyId.title",
                )}
                description={t(
                  "settings.postProcessing.api.bedrock.accessKeyId.title",
                )}
                descriptionMode="tooltip"
                layout="horizontal"
                grouped={true}
              >
                <Input
                  type="text"
                  defaultValue={state.bedrockAccessKeyId}
                  onBlur={(e) =>
                    state.handleBedrockSettingChange(
                      "access_key_id",
                      e.target.value.trim(),
                    )
                  }
                  placeholder={t(
                    "settings.postProcessing.api.bedrock.accessKeyId.placeholder",
                  )}
                  variant="compact"
                  className="min-w-[200px]"
                />
              </SettingContainer>
              <SettingContainer
                title={t(
                  "settings.postProcessing.api.bedrock.secretAccessKey.title",
                )}
                description={t(
                  "settings.postProcessing.api.bedrock.secretAccessKey.title",
                )}
                descriptionMode="tooltip"
                layout="horizontal"
                grouped={true}
              >
                <Input
                  type="password"
                  defaultValue={state.bedrockSecretAccessKey}
                  onBlur={(e) =>
                    state.handleBedrockSettingChange(
                      "secret_access_key",
                      e.target.value.trim(),
                    )
                  }
                  placeholder={t(
                    "settings.postProcessing.api.bedrock.secretAccessKey.placeholder",
                  )}
                  variant="compact"
                  className="min-w-[200px]"
                />
              </SettingContainer>
              <SettingContainer
                title={t(
                  "settings.postProcessing.api.bedrock.sessionToken.title",
                )}
                description={t(
                  "settings.postProcessing.api.bedrock.sessionToken.description",
                )}
                descriptionMode="tooltip"
                layout="horizontal"
                grouped={true}
              >
                <Input
                  type="password"
                  defaultValue={state.bedrockSessionToken}
                  onBlur={(e) =>
                    state.handleBedrockSettingChange(
                      "session_token",
                      e.target.value.trim(),
                    )
                  }
                  placeholder={t(
                    "settings.postProcessing.api.bedrock.sessionToken.placeholder",
                  )}
                  variant="compact"
                  className="min-w-[200px]"
                />
              </SettingContainer>
            </>
          )}

          <SettingContainer
            title={t("settings.postProcessing.api.bedrock.region.title")}
            description=""
            layout="horizontal"
            grouped={true}
          >
            <Dropdown
              options={[
                { value: "us-east-1", label: "us-east-1" },
                { value: "us-east-2", label: "us-east-2" },
                { value: "us-west-2", label: "us-west-2" },
                { value: "eu-west-1", label: "eu-west-1" },
                { value: "eu-west-2", label: "eu-west-2" },
                { value: "eu-west-3", label: "eu-west-3" },
                { value: "eu-central-1", label: "eu-central-1" },
                { value: "eu-north-1", label: "eu-north-1" },
                { value: "ap-southeast-1", label: "ap-southeast-1" },
                { value: "ap-southeast-2", label: "ap-southeast-2" },
                { value: "ap-northeast-1", label: "ap-northeast-1" },
                { value: "ap-northeast-2", label: "ap-northeast-2" },
                { value: "ap-south-1", label: "ap-south-1" },
                { value: "ca-central-1", label: "ca-central-1" },
                { value: "sa-east-1", label: "sa-east-1" },
              ]}
              selectedValue={state.bedrockRegion}
              onSelect={(v) => state.handleBedrockSettingChange("region", v)}
              className="flex-1"
            />
          </SettingContainer>

          <SettingContainer
            title={t("settings.postProcessing.api.bedrock.crossRegion.title")}
            description={t(
              "settings.postProcessing.api.bedrock.crossRegion.description",
            )}
            descriptionMode="tooltip"
            layout="horizontal"
            grouped={true}
          >
            <input
              type="checkbox"
              checked={state.bedrockUseCrossRegion}
              onChange={(e) =>
                state.handleBedrockSettingChange(
                  "use_cross_region",
                  e.target.checked ? "true" : "false",
                )
              }
              className="h-4 w-4"
            />
          </SettingContainer>
        </>
      ) : (
        <>
          {state.selectedProvider?.id === "custom" && (
            <SettingContainer
              title={t("settings.postProcessing.api.baseUrl.title")}
              description={t("settings.postProcessing.api.baseUrl.description")}
              descriptionMode="tooltip"
              layout="horizontal"
              grouped={true}
            >
              <div className="flex items-center gap-2">
                <BaseUrlField
                  value={state.baseUrl}
                  onBlur={state.handleBaseUrlChange}
                  placeholder={t(
                    "settings.postProcessing.api.baseUrl.placeholder",
                  )}
                  disabled={state.isBaseUrlUpdating}
                  className="min-w-[380px]"
                />
              </div>
            </SettingContainer>
          )}

          <SettingContainer
            title={t("settings.postProcessing.api.apiKey.title")}
            description={t("settings.postProcessing.api.apiKey.description")}
            descriptionMode="tooltip"
            layout="horizontal"
            grouped={true}
          >
            <div className="flex items-center gap-2">
              <ApiKeyField
                value={state.apiKey}
                onBlur={state.handleApiKeyChange}
                placeholder={t(
                  "settings.postProcessing.api.apiKey.placeholder",
                )}
                disabled={state.isApiKeyUpdating}
                className="min-w-[320px]"
              />
            </div>
          </SettingContainer>
        </>
      )}

      {!state.isAppleProvider && (
        <SettingContainer
          title={t("settings.postProcessing.api.model.title")}
          description={
            state.isCustomProvider
              ? t("settings.postProcessing.api.model.descriptionCustom")
              : t("settings.postProcessing.api.model.descriptionDefault")
          }
          descriptionMode="tooltip"
          layout="stacked"
          grouped={true}
        >
          <div className="flex items-center gap-2">
            <ModelSelect
              value={state.model}
              options={state.modelOptions}
              disabled={state.isModelUpdating}
              isLoading={state.isFetchingModels}
              placeholder={
                state.modelOptions.length > 0
                  ? t(
                      "settings.postProcessing.api.model.placeholderWithOptions",
                    )
                  : t("settings.postProcessing.api.model.placeholderNoOptions")
              }
              onSelect={state.handleModelSelect}
              onCreate={state.handleModelCreate}
              onBlur={() => {}}
              onMenuOpen={
                state.isBedrockProvider ? state.handleRefreshModels : undefined
              }
              className="flex-1 min-w-[380px]"
            />
            {!state.isBedrockProvider && (
              <ResetButton
                onClick={state.handleRefreshModels}
                disabled={state.isFetchingModels}
                ariaLabel={t("settings.postProcessing.api.model.refreshModels")}
                className="flex h-10 w-10 items-center justify-center"
              >
                <RefreshCcw
                  className={`h-4 w-4 ${state.isFetchingModels ? "animate-spin" : ""}`}
                />
              </ResetButton>
            )}
          </div>
          {state.modelFetchError && (
            <p className="text-sm text-red-500 mt-1">{state.modelFetchError}</p>
          )}
        </SettingContainer>
      )}

      {state.isBedrockProvider && (
        <SettingContainer
          title={t("settings.postProcessing.api.bedrock.testConnection.title")}
          description={t(
            "settings.postProcessing.api.bedrock.testConnection.description",
          )}
          descriptionMode="tooltip"
          layout="horizontal"
          grouped={true}
        >
          <div
            title={
              !state.model
                ? t(
                    "settings.postProcessing.api.bedrock.testConnection.selectModel",
                  )
                : ""
            }
          >
            <Button
              onClick={state.handleBedrockTestConnection}
              variant="secondary"
              size="md"
              disabled={
                state.bedrockTestResult.status === "testing" || !state.model
              }
              className={
                state.bedrockTestResult.status === "success"
                  ? "border-green-500 text-green-500"
                  : ""
              }
            >
              {state.bedrockTestResult.status === "testing"
                ? t(
                    "settings.postProcessing.api.bedrock.testConnection.testing",
                  )
                : state.bedrockTestResult.status === "success"
                  ? `✓ ${t("settings.postProcessing.api.bedrock.testConnection.success")}`
                  : t(
                      "settings.postProcessing.api.bedrock.testConnection.button",
                    )}
            </Button>
          </div>
          {state.bedrockTestResult.status === "error" && (
            <p className="text-sm text-red-500 mt-1">
              {state.bedrockTestResult.message}
            </p>
          )}
        </SettingContainer>
      )}
    </>
  );
};

const PostProcessingSettingsPromptsComponent: React.FC = () => {
  const { t } = useTranslation();
  const { getSetting, updateSetting, isUpdating, refreshSettings } =
    useSettings();
  const [isCreating, setIsCreating] = useState(false);
  const [draftName, setDraftName] = useState("");
  const [draftText, setDraftText] = useState("");

  const prompts = getSetting("post_process_prompts") || [];
  const selectedPromptId = getSetting("post_process_selected_prompt_id") || "";
  const selectedPrompt =
    prompts.find((prompt) => prompt.id === selectedPromptId) || null;

  useEffect(() => {
    if (isCreating) return;

    if (selectedPrompt) {
      setDraftName(selectedPrompt.name);
      setDraftText(selectedPrompt.prompt);
    } else {
      setDraftName("");
      setDraftText("");
    }
  }, [
    isCreating,
    selectedPromptId,
    selectedPrompt?.name,
    selectedPrompt?.prompt,
  ]);

  const handlePromptSelect = (promptId: string | null) => {
    if (!promptId) return;
    updateSetting("post_process_selected_prompt_id", promptId);
    setIsCreating(false);
  };

  const handleCreatePrompt = async () => {
    if (!draftName.trim() || !draftText.trim()) return;

    try {
      const result = await commands.addPostProcessPrompt(
        draftName.trim(),
        draftText.trim(),
      );
      if (result.status === "ok") {
        await refreshSettings();
        updateSetting("post_process_selected_prompt_id", result.data.id);
        setIsCreating(false);
      }
    } catch (error) {
      console.error("Failed to create prompt:", error);
    }
  };

  const handleUpdatePrompt = async () => {
    if (!selectedPromptId || !draftName.trim() || !draftText.trim()) return;

    try {
      await commands.updatePostProcessPrompt(
        selectedPromptId,
        draftName.trim(),
        draftText.trim(),
      );
      await refreshSettings();
    } catch (error) {
      console.error("Failed to update prompt:", error);
    }
  };

  const handleDeletePrompt = async (promptId: string) => {
    if (!promptId) return;

    try {
      await commands.deletePostProcessPrompt(promptId);
      await refreshSettings();
      setIsCreating(false);
    } catch (error) {
      console.error("Failed to delete prompt:", error);
    }
  };

  const handleCancelCreate = () => {
    setIsCreating(false);
    if (selectedPrompt) {
      setDraftName(selectedPrompt.name);
      setDraftText(selectedPrompt.prompt);
    } else {
      setDraftName("");
      setDraftText("");
    }
  };

  const handleStartCreate = () => {
    setIsCreating(true);
    setDraftName("");
    setDraftText("");
  };

  const hasPrompts = prompts.length > 0;
  const isDirty =
    !!selectedPrompt &&
    (draftName.trim() !== selectedPrompt.name ||
      draftText.trim() !== selectedPrompt.prompt.trim());

  return (
    <SettingContainer
      title={t("settings.postProcessing.prompts.selectedPrompt.title")}
      description={t(
        "settings.postProcessing.prompts.selectedPrompt.description",
      )}
      descriptionMode="tooltip"
      layout="stacked"
      grouped={true}
    >
      <div className="space-y-3">
        <div className="flex gap-2">
          <Dropdown
            selectedValue={selectedPromptId || null}
            options={prompts.map((p) => ({
              value: p.id,
              label: p.name,
            }))}
            onSelect={(value) => handlePromptSelect(value)}
            placeholder={
              prompts.length === 0
                ? t("settings.postProcessing.prompts.noPrompts")
                : t("settings.postProcessing.prompts.selectPrompt")
            }
            disabled={
              isUpdating("post_process_selected_prompt_id") || isCreating
            }
            className="flex-1"
          />
          <Button
            onClick={handleStartCreate}
            variant="primary"
            size="md"
            disabled={isCreating}
          >
            {t("settings.postProcessing.prompts.createNew")}
          </Button>
        </div>

        {!isCreating && hasPrompts && selectedPrompt && (
          <div className="space-y-3">
            <div className="space-y-2 flex flex-col">
              <label className="text-sm font-semibold">
                {t("settings.postProcessing.prompts.promptLabel")}
              </label>
              <Input
                type="text"
                value={draftName}
                onChange={(e) => setDraftName(e.target.value)}
                placeholder={t(
                  "settings.postProcessing.prompts.promptLabelPlaceholder",
                )}
                variant="compact"
              />
            </div>

            <div className="space-y-2 flex flex-col">
              <label className="text-sm font-semibold">
                {t("settings.postProcessing.prompts.promptInstructions")}
              </label>
              <Textarea
                value={draftText}
                onChange={(e) => setDraftText(e.target.value)}
                placeholder={t(
                  "settings.postProcessing.prompts.promptInstructionsPlaceholder",
                )}
              />
              <p
                className="text-xs text-mid-gray/70"
                dangerouslySetInnerHTML={{
                  __html: t("settings.postProcessing.prompts.promptTip"),
                }}
              />
            </div>

            <div className="flex gap-2 pt-2">
              <Button
                onClick={handleUpdatePrompt}
                variant="primary"
                size="md"
                disabled={!draftName.trim() || !draftText.trim() || !isDirty}
              >
                {t("settings.postProcessing.prompts.updatePrompt")}
              </Button>
              <Button
                onClick={() => handleDeletePrompt(selectedPromptId)}
                variant="secondary"
                size="md"
                disabled={!selectedPromptId || prompts.length <= 1}
              >
                {t("settings.postProcessing.prompts.deletePrompt")}
              </Button>
            </div>
          </div>
        )}

        {!isCreating && !selectedPrompt && (
          <div className="p-3 bg-mid-gray/5 rounded-md border border-mid-gray/20">
            <p className="text-sm text-mid-gray">
              {hasPrompts
                ? t("settings.postProcessing.prompts.selectToEdit")
                : t("settings.postProcessing.prompts.createFirst")}
            </p>
          </div>
        )}

        {isCreating && (
          <div className="space-y-3">
            <div className="space-y-2 block flex flex-col">
              <label className="text-sm font-semibold text-text">
                {t("settings.postProcessing.prompts.promptLabel")}
              </label>
              <Input
                type="text"
                value={draftName}
                onChange={(e) => setDraftName(e.target.value)}
                placeholder={t(
                  "settings.postProcessing.prompts.promptLabelPlaceholder",
                )}
                variant="compact"
              />
            </div>

            <div className="space-y-2 flex flex-col">
              <label className="text-sm font-semibold">
                {t("settings.postProcessing.prompts.promptInstructions")}
              </label>
              <Textarea
                value={draftText}
                onChange={(e) => setDraftText(e.target.value)}
                placeholder={t(
                  "settings.postProcessing.prompts.promptInstructionsPlaceholder",
                )}
              />
              <p
                className="text-xs text-mid-gray/70"
                dangerouslySetInnerHTML={{
                  __html: t("settings.postProcessing.prompts.promptTip"),
                }}
              />
            </div>

            <div className="flex gap-2 pt-2">
              <Button
                onClick={handleCreatePrompt}
                variant="primary"
                size="md"
                disabled={!draftName.trim() || !draftText.trim()}
              >
                {t("settings.postProcessing.prompts.createPrompt")}
              </Button>
              <Button
                onClick={handleCancelCreate}
                variant="secondary"
                size="md"
              >
                {t("settings.postProcessing.prompts.cancel")}
              </Button>
            </div>
          </div>
        )}
      </div>
    </SettingContainer>
  );
};

export const PostProcessingSettingsApi = React.memo(
  PostProcessingSettingsApiComponent,
);
PostProcessingSettingsApi.displayName = "PostProcessingSettingsApi";

export const PostProcessingSettingsPrompts = React.memo(
  PostProcessingSettingsPromptsComponent,
);
PostProcessingSettingsPrompts.displayName = "PostProcessingSettingsPrompts";

export const PostProcessingSettings: React.FC = () => {
  const { t } = useTranslation();

  return (
    <div className="max-w-3xl w-full mx-auto space-y-6">
      <SettingsGroup title={t("settings.postProcessing.hotkey.title")}>
        <ShortcutInput
          shortcutId="transcribe_with_post_process"
          descriptionMode="tooltip"
          grouped={true}
        />
      </SettingsGroup>

      <SettingsGroup title={t("settings.postProcessing.api.title")}>
        <PostProcessingSettingsApi />
      </SettingsGroup>

      <SettingsGroup title={t("settings.postProcessing.prompts.title")}>
        <PostProcessingSettingsPrompts />
      </SettingsGroup>
    </div>
  );
};

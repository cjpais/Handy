import React, { useEffect, useState } from "react";
import { Trans, useTranslation } from "react-i18next";
import { RefreshCcw } from "lucide-react";
import { commands, type PostProcessApiFormat } from "@/bindings";

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
import { ContextScribeToggle } from "../ContextScribeToggle";
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
      ) : (
        <>
          {state.isCustomProvider && (
            <SettingContainer
              title={t("settings.postProcessing.api.apiFormat.title")}
              description={t(
                "settings.postProcessing.api.apiFormat.description",
              )}
              descriptionMode="tooltip"
              layout="horizontal"
              grouped={true}
            >
              <Dropdown
                options={[
                  {
                    value: "openai",
                    label: t(
                      "settings.postProcessing.api.apiFormat.options.openai",
                    ),
                  },
                  {
                    value: "anthropic",
                    label: t(
                      "settings.postProcessing.api.apiFormat.options.anthropic",
                    ),
                  },
                ]}
                selectedValue={state.apiFormat}
                onSelect={(value) =>
                  state.handleApiFormatChange(value as PostProcessApiFormat)
                }
                disabled={state.isApiFormatUpdating}
              />
            </SettingContainer>
          )}

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
                  placeholder={
                    state.apiFormat === "anthropic"
                      ? t(
                          "settings.postProcessing.api.baseUrl.placeholderAnthropic",
                        )
                      : t("settings.postProcessing.api.baseUrl.placeholder")
                  }
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
              className="flex-1 min-w-[380px]"
            />
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
          </div>
        </SettingContainer>
      )}
    </>
  );
};

const PromptLibraryEditor: React.FC<{ kind: "postProcess" | "contextScribe" }> = ({
  kind,
}) => {
  const { t } = useTranslation();
  const { getSetting, updateSetting, isUpdating, refreshSettings } =
    useSettings();
  const [isCreating, setIsCreating] = useState(false);
  const [draftName, setDraftName] = useState("");
  const [draftText, setDraftText] = useState("");
  const scribe = kind === "contextScribe";
  const promptsKey = scribe
    ? "context_scribe_prompts"
    : "post_process_prompts";
  const selectedKey = scribe
    ? "context_scribe_selected_prompt_id"
    : "post_process_selected_prompt_id";

  const prompts = getSetting(promptsKey) || [];
  const selectedPromptId = getSetting(selectedKey) || "";
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
    updateSetting(selectedKey, promptId);
    setIsCreating(false);
  };

  const handleCreatePrompt = async () => {
    if (!draftName.trim() || !draftText.trim()) return;

    try {
      const result = scribe
        ? await commands.addContextScribePrompt(draftName.trim(), draftText.trim())
        : await commands.addPostProcessPrompt(draftName.trim(), draftText.trim());
      if (result.status === "ok") {
        await refreshSettings();
        updateSetting(selectedKey, result.data.id);
        setIsCreating(false);
      }
    } catch (error) {
      console.error("Failed to create prompt:", error);
    }
  };

  const handleUpdatePrompt = async () => {
    if (!selectedPromptId || !draftName.trim() || !draftText.trim()) return;

    try {
      if (scribe) {
        await commands.updateContextScribePrompt(
          selectedPromptId,
          draftName.trim(),
          draftText.trim(),
        );
      } else {
        await commands.updatePostProcessPrompt(
          selectedPromptId,
          draftName.trim(),
          draftText.trim(),
        );
      }
      await refreshSettings();
    } catch (error) {
      console.error("Failed to update prompt:", error);
    }
  };

  const handleDeletePrompt = async (promptId: string) => {
    if (!promptId) return;

    try {
      if (scribe) {
        await commands.deleteContextScribePrompt(promptId);
      } else {
        await commands.deletePostProcessPrompt(promptId);
      }
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
      title={t(
        scribe
          ? "settings.contextScribe.prompts.selectedPrompt.title"
          : "settings.postProcessing.prompts.selectedPrompt.title",
      )}
      description={t(
        scribe
          ? "settings.contextScribe.prompts.selectedPrompt.description"
          : "settings.postProcessing.prompts.selectedPrompt.description",
      )}
      descriptionMode="tooltip"
      layout="stacked"
      grouped={true}
    >
      <div className="space-y-3">
        <div className="flex gap-2 min-w-0">
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
              isUpdating(selectedKey) || isCreating
            }
            className="flex-1 min-w-0"
          />
          <Button
            onClick={handleStartCreate}
            variant="primary"
            size="md"
            disabled={isCreating}
            className="shrink-0"
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
              <p className="text-xs text-ink-3">
                {scribe ? (
                  <Trans
                    i18nKey="settings.contextScribe.prompts.promptTip"
                    components={{
                      output: <code />,
                      selection: <code />,
                    }}
                  />
                ) : (
                  <Trans
                    i18nKey="settings.postProcessing.prompts.promptTip"
                    components={{ code: <code /> }}
                  />
                )}
              </p>
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
          <div className="p-3 bg-wash rounded-md border border-line">
            <p className="text-sm text-ink-3">
              {hasPrompts
                ? t("settings.postProcessing.prompts.selectToEdit")
                : t(
                    scribe
                      ? "settings.contextScribe.prompts.createFirst"
                      : "settings.postProcessing.prompts.createFirst",
                  )}
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
              <p className="text-xs text-ink-3">
                {scribe ? (
                  <Trans
                    i18nKey="settings.contextScribe.prompts.promptTip"
                    components={{
                      output: <code />,
                      selection: <code />,
                    }}
                  />
                ) : (
                  <Trans
                    i18nKey="settings.postProcessing.prompts.promptTip"
                    components={{ code: <code /> }}
                  />
                )}
              </p>
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

const PostProcessingSettingsPromptsComponent: React.FC = () => (
  <PromptLibraryEditor kind="postProcess" />
);

export const PostProcessingSettingsPrompts = React.memo(
  PostProcessingSettingsPromptsComponent,
);
PostProcessingSettingsPrompts.displayName = "PostProcessingSettingsPrompts";

const ContextScribeMaxSelection: React.FC = () => {
  const { t } = useTranslation();
  const { getSetting, updateSetting, isUpdating } = useSettings();
  const limit = getSetting("context_scribe_max_selection_chars") ?? 2000;

  const handleChange = (event: React.ChangeEvent<HTMLInputElement>) => {
    const value = parseInt(event.target.value, 10);
    if (!isNaN(value) && value >= 0) {
      updateSetting("context_scribe_max_selection_chars", value);
    }
  };

  return (
    <SettingContainer
      title={t("settings.contextScribe.maxSelection.title")}
      description={t("settings.contextScribe.maxSelection.description")}
      descriptionMode="tooltip"
      grouped={true}
      layout="horizontal"
    >
      <div className="flex items-center space-x-2">
        <Input
          type="number"
          min="0"
          value={limit}
          onChange={handleChange}
          disabled={isUpdating("context_scribe_max_selection_chars")}
          className="w-24"
        />
        <span className="text-sm text-text">
          {t("settings.contextScribe.maxSelection.characters")}
        </span>
      </div>
    </SettingContainer>
  );
};

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
        <ContextScribeToggle descriptionMode="tooltip" grouped={true} />
        <ShortcutInput
          shortcutId="transcribe_with_context_scribe"
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

      <SettingsGroup title={t("settings.contextScribe.title")}>
        <ContextScribeMaxSelection />
        <PromptLibraryEditor kind="contextScribe" />
      </SettingsGroup>
    </div>
  );
};

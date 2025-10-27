import React, { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { RefreshCcw } from "lucide-react";

import { SettingsGroup } from "../../ui/SettingsGroup";
import { SettingContainer } from "../../ui/SettingContainer";
import { Button } from "../../ui/Button";
import { ResetButton } from "../../ui/ResetButton";
import { Input } from "../../ui/Input";
import { Select } from "../../ui/Select";
import { Textarea } from "../../ui/Textarea";

import { ProviderSelect } from "../PostProcessingSettingsApi/ProviderSelect";
import { BaseUrlField } from "../PostProcessingSettingsApi/BaseUrlField";
import { ApiKeyField } from "../PostProcessingSettingsApi/ApiKeyField";
import { ModelSelect } from "../PostProcessingSettingsApi/ModelSelect";
import { usePostProcessProviderState } from "../PostProcessingSettingsApi/usePostProcessProviderState";
import { useSettings } from "../../../hooks/useSettings";
import type { LLMPrompt } from "../../../lib/types";

const DisabledNotice: React.FC<{ children: React.ReactNode }> = ({ children }) => (
  <div className="p-4 bg-mid-gray/5 rounded-lg border border-mid-gray/20 text-center">
    <p className="text-sm text-mid-gray">{children}</p>
  </div>
);

const PostProcessingSettingsApiComponent: React.FC = () => {
  const state = usePostProcessProviderState();

  if (!state.enabled) {
    return (
      <DisabledNotice>
        Post processing is currently disabled. Enable it in Debug settings to configure.
      </DisabledNotice>
    );
  }

  return (
    <>
      <SettingContainer
        title="Provider"
        description="Select an OpenAI-compatible provider."
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

      <SettingContainer
        title="Base URL"
        description="API base URL for the selected provider. Only the custom provider can be edited."
        descriptionMode="tooltip"
        layout="horizontal"
        grouped={true}
      >
        <div className="flex items-center gap-2">
          <BaseUrlField
            value={state.baseUrl}
            onChange={state.setBaseUrl}
            onBlur={state.commitBaseUrl}
            placeholder="https://api.openai.com/v1"
            disabled={
              !state.selectedProvider?.allow_base_url_edit ||
              state.isBaseUrlUpdating
            }
            className="min-w-[380px]"
          />
        </div>
      </SettingContainer>

      <SettingContainer
        title="API Key"
        description="API key for the selected provider."
        descriptionMode="tooltip"
        layout="horizontal"
        grouped={true}
      >
        <div className="flex items-center gap-2">
          <ApiKeyField
            value={state.apiKey}
            onChange={state.setApiKey}
            onBlur={state.commitApiKey}
            placeholder="sk-..."
            disabled={state.isApiKeyUpdating}
            className="min-w-[320px]"
          />
        </div>
      </SettingContainer>

      <SettingContainer
        title="Model"
        description={
          state.isCustomProvider
            ? "Provide the model identifier expected by your custom endpoint."
            : "Choose a model exposed by the selected provider."
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
                ? "Search or select a model"
                : "Type a model name"
            }
            onSelect={state.handleModelSelect}
            onCreate={state.handleModelCreate}
            onBlur={state.commitModel}
            className="flex-1 min-w-[380px]"
          />
          <ResetButton
            onClick={state.handleRefreshModels}
            disabled={state.isFetchingModels}
            ariaLabel="Refresh models"
            className="flex h-10 w-10 items-center justify-center"
          >
            <RefreshCcw
              className={`h-4 w-4 ${state.isFetchingModels ? "animate-spin" : ""}`}
            />
          </ResetButton>
        </div>
      </SettingContainer>
    </>
  );
};

const PostProcessingSettingsPromptsComponent: React.FC = () => {
  const { getSetting, updateSetting, isUpdating, refreshSettings } = useSettings();
  const [isCreating, setIsCreating] = useState(false);
  const [draftName, setDraftName] = useState("");
  const [draftText, setDraftText] = useState("");

  const enabled = getSetting("post_process_enabled") || false;
  const prompts = getSetting("post_process_prompts") || [];
  const selectedPromptId = getSetting("post_process_selected_prompt_id") || "";
  const selectedPrompt = prompts.find((prompt) => prompt.id === selectedPromptId) || null;

  useEffect(() => {
    if (isCreating) return;

    if (selectedPrompt) {
      setDraftName(selectedPrompt.name);
      setDraftText(selectedPrompt.prompt);
    } else {
      setDraftName("");
      setDraftText("");
    }
  }, [isCreating, selectedPromptId, selectedPrompt?.name, selectedPrompt?.prompt]);

  const handlePromptSelect = (promptId: string | null) => {
    if (!promptId) return;
    updateSetting("post_process_selected_prompt_id", promptId);
    setIsCreating(false);
  };

  const handleCreatePrompt = async () => {
    if (!draftName.trim() || !draftText.trim()) return;

    try {
      const newPrompt = await invoke<LLMPrompt>("add_post_process_prompt", {
        name: draftName.trim(),
        prompt: draftText.trim(),
      });
      await refreshSettings();
      updateSetting("post_process_selected_prompt_id", newPrompt.id);
      setIsCreating(false);
    } catch (error) {
      console.error("Failed to create prompt:", error);
    }
  };

  const handleUpdatePrompt = async () => {
    if (!selectedPromptId || !draftName.trim() || !draftText.trim()) return;

    try {
      await invoke("update_post_process_prompt", {
        id: selectedPromptId,
        name: draftName.trim(),
        prompt: draftText.trim(),
      });
      await refreshSettings();
    } catch (error) {
      console.error("Failed to update prompt:", error);
    }
  };

  const handleDeletePrompt = async (promptId: string) => {
    if (!promptId) return;

    try {
      await invoke("delete_post_process_prompt", { id: promptId });
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

  if (!enabled) {
    return (
      <DisabledNotice>
        Post processing is currently disabled. Enable it in Debug settings to configure.
      </DisabledNotice>
    );
  }

  const hasPrompts = prompts.length > 0;
  const isDirty =
    !!selectedPrompt &&
    (draftName.trim() !== selectedPrompt.name ||
      draftText.trim() !== selectedPrompt.prompt.trim());

  return (
    <SettingContainer
      title="Post-processing Prompt"
      description="Select a template for refining transcriptions or create a new one. Use ${output} inside the prompt text to reference the captured transcript."
      descriptionMode="tooltip"
      layout="stacked"
      grouped={true}
    >
      <div className="space-y-3">
        <Select
          value={selectedPromptId || null}
          options={prompts.map((p) => ({
            value: p.id,
            label: p.name,
          }))}
          onChange={(value) => handlePromptSelect(value)}
          onCreateOption={(label) => {
            const trimmed = label.trim();
            if (!trimmed) return;
            setIsCreating(true);
            setDraftName(trimmed);
            setDraftText("");
          }}
          placeholder={
            prompts.length === 0
              ? "Type a name to create the first prompt"
              : "Search or create a prompt"
          }
          disabled={isUpdating("post_process_selected_prompt_id")}
          isLoading={isUpdating("post_process_selected_prompt_id")}
          isCreatable
          className="w-full"
        />

        {!isCreating && hasPrompts && selectedPrompt && (
          <div className="space-y-3">
            <div className="space-y-2 block flex flex-col">
              <label className="text-sm font-semibold text-mid-gray">
                Prompt Label
              </label>
              <Input
                type="text"
                value={draftName}
                onChange={(e) => setDraftName(e.target.value)}
                placeholder="Enter prompt name"
                variant="compact"
              />
            </div>

            <div className="space-y-2 block flex flex-col">
              <label className="text-sm font-semibold text-mid-gray">
                Prompt Instructions
              </label>
              <Textarea
                value={draftText}
                onChange={(e) => setDraftText(e.target.value)}
                placeholder="Write the instructions to run after transcription. Example: Improve grammar and clarity for the following text: ${output}"
              />
              <p className="text-xs text-mid-gray/70">
                Tip: Use <code className="px-1 py-0.5 bg-mid-gray/20 rounded text-xs">$&#123;output&#125;</code> to insert the transcribed text in your prompt.
              </p>
            </div>

            <div className="flex gap-2 pt-2">
              <Button
                onClick={handleUpdatePrompt}
                variant="primary"
                size="md"
                disabled={!draftName.trim() || !draftText.trim() || !isDirty}
              >
                Update Prompt
              </Button>
              <Button
                onClick={() => handleDeletePrompt(selectedPromptId)}
                variant="secondary"
                size="md"
                disabled={!selectedPromptId || prompts.length <= 1}
              >
                Delete Prompt
              </Button>
            </div>
          </div>
        )}

        {!isCreating && !selectedPrompt && (
          <div className="p-3 bg-mid-gray/5 rounded border border-mid-gray/20">
            <p className="text-sm text-mid-gray">
              {hasPrompts
                ? "Select a prompt above to view and edit its details."
                : "Type a name above to create your first post-processing prompt."}
            </p>
          </div>
        )}

        {isCreating && (
          <div className="space-y-3">
            <div className="space-y-2 block flex flex-col">
              <label className="text-sm font-semibold text-mid-gray">
                Prompt Label
              </label>
              <Input
                type="text"
                value={draftName}
                onChange={(e) => setDraftName(e.target.value)}
                placeholder="Enter prompt name"
                variant="compact"
              />
            </div>

            <div className="space-y-2 block flex flex-col">
              <label className="text-sm font-semibold text-mid-gray">
                Prompt Instructions
              </label>
              <Textarea
                value={draftText}
                onChange={(e) => setDraftText(e.target.value)}
                placeholder="Write the instructions to run after transcription. Example: Improve grammar and clarity for the following text: ${output}"
              />
              <p className="text-xs text-mid-gray/70">
                Tip: Use <code className="px-1 py-0.5 bg-mid-gray/20 rounded text-xs">$&#123;output&#125;</code> to insert the transcribed text in your prompt.
              </p>
            </div>

            <div className="flex gap-2 pt-2">
              <Button
                onClick={handleCreatePrompt}
                variant="primary"
                size="md"
                disabled={!draftName.trim() || !draftText.trim()}
              >
                Create Prompt
              </Button>
              <Button
                onClick={handleCancelCreate}
                variant="secondary"
                size="md"
              >
                Cancel
              </Button>
            </div>
          </div>
        )}
      </div>
    </SettingContainer>
  );
};

export const PostProcessingSettingsApi = React.memo(PostProcessingSettingsApiComponent);
PostProcessingSettingsApi.displayName = "PostProcessingSettingsApi";

export const PostProcessingSettingsPrompts = React.memo(PostProcessingSettingsPromptsComponent);
PostProcessingSettingsPrompts.displayName = "PostProcessingSettingsPrompts";

export const PostProcessingSettings: React.FC = () => {
  return (
    <div className="max-w-3xl w-full mx-auto space-y-6">
      <SettingsGroup title="API (OpenAI Compatible)">
        <PostProcessingSettingsApi />
      </SettingsGroup>

      <SettingsGroup title="Prompt">
        <PostProcessingSettingsPrompts />
      </SettingsGroup>
    </div>
  );
};

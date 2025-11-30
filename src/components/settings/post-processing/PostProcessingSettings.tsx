import React, { useEffect, useState } from "react";
import { RefreshCcw } from "lucide-react";
import { commands } from "@/bindings";

import { SettingsGroup } from "../../ui/SettingsGroup";
import { SettingContainer } from "../../ui/SettingContainer";
import { Button } from "../../ui/Button";
import { ResetButton } from "../../ui/ResetButton";
import { Input } from "../../ui/Input";
import { Dropdown } from "../../ui/Dropdown";
import { Textarea } from "../../ui/Textarea";

import { ProviderSelect } from "../PostProcessingSettingsApi/ProviderSelect";
import { BaseUrlField } from "../PostProcessingSettingsApi/BaseUrlField";
import { ApiKeyField } from "../PostProcessingSettingsApi/ApiKeyField";
import { ModelSelect } from "../PostProcessingSettingsApi/ModelSelect";
import { usePostProcessProviderState } from "../PostProcessingSettingsApi/usePostProcessProviderState";
import { useSettings } from "../../../hooks/useSettings";
import type { LLMPrompt } from "@/bindings";

const DisabledNotice: React.FC<{ children: React.ReactNode }> = ({
  children,
}) => (
  <div className="p-4 bg-mid-gray/5 rounded-lg border border-mid-gray/20 text-center">
    <p className="text-sm text-mid-gray">{children}</p>
  </div>
);

const PostProcessingSettingsApiComponent: React.FC = () => {
  const state = usePostProcessProviderState();

  if (!state.enabled) {
    return (
      <DisabledNotice>
        Post processing is currently disabled. Enable it in Debug settings to
        configure.
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
            onBlur={state.handleBaseUrlChange}
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
            onBlur={state.handleApiKeyChange}
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
            onBlur={() => {}}
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
  const { getSetting, updateSetting, isUpdating, refreshSettings } =
    useSettings();
  const [isCreating, setIsCreating] = useState(false);
  const [draftName, setDraftName] = useState("");
  const [draftText, setDraftText] = useState("");

  const enabled = getSetting("post_process_enabled") || false;
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

  if (!enabled) {
    return (
      <DisabledNotice>
        Post processing is currently disabled. Enable it in Debug settings to
        configure.
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
      title="Selected Prompt"
      description="Select a template for refining transcriptions or create a new one. Use ${output} inside the prompt text to reference the captured transcript."
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
              prompts.length === 0 ? "No prompts available" : "Select a prompt"
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
            Create New Prompt
          </Button>
        </div>

        {!isCreating && hasPrompts && selectedPrompt && (
          <div className="space-y-3">
            <div className="space-y-2 flex flex-col">
              <label className="text-sm font-semibold">Prompt Label</label>
              <Input
                type="text"
                value={draftName}
                onChange={(e) => setDraftName(e.target.value)}
                placeholder="Enter prompt name"
                variant="compact"
              />
            </div>

            <div className="space-y-2 flex flex-col">
              <label className="text-sm font-semibold">
                Prompt Instructions
              </label>
              <Textarea
                value={draftText}
                onChange={(e) => setDraftText(e.target.value)}
                placeholder="Write the instructions to run after transcription. Example: Improve grammar and clarity for the following text: ${output}"
              />
              <p className="text-xs text-mid-gray/70">
                Tip: Use{" "}
                <code className="px-1 py-0.5 bg-mid-gray/20 rounded text-xs">
                  $&#123;output&#125;
                </code>{" "}
                to insert the transcribed text in your prompt.
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
                : "Click 'Create New Prompt' above to create your first post-processing prompt."}
            </p>
          </div>
        )}

        {isCreating && (
          <div className="space-y-3">
            <div className="space-y-2 block flex flex-col">
              <label className="text-sm font-semibold text-text">
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

            <div className="space-y-2 flex flex-col">
              <label className="text-sm font-semibold">
                Prompt Instructions
              </label>
              <Textarea
                value={draftText}
                onChange={(e) => setDraftText(e.target.value)}
                placeholder="Write the instructions to run after transcription. Example: Improve grammar and clarity for the following text: ${output}"
              />
              <p className="text-xs text-mid-gray/70">
                Tip: Use{" "}
                <code className="px-1 py-0.5 bg-mid-gray/20 rounded text-xs">
                  $&#123;output&#125;
                </code>{" "}
                to insert the transcribed text in your prompt.
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

export const PostProcessingSettingsApi = React.memo(
  PostProcessingSettingsApiComponent,
);
PostProcessingSettingsApi.displayName = "PostProcessingSettingsApi";

export const PostProcessingSettingsPrompts = React.memo(
  PostProcessingSettingsPromptsComponent,
);
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

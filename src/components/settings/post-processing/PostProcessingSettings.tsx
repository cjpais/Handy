import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { commands } from "@/bindings";

import { SettingsGroup } from "../../ui/SettingsGroup";
import { SettingContainer } from "../../ui/SettingContainer";
import { Button } from "../../ui/Button";
import { Input } from "../../ui/Input";
import { Dropdown } from "../../ui/Dropdown";
import { Textarea } from "../../ui/Textarea";
import { Modal } from "../../ui/Modal";

import { ProviderSelect } from "../PostProcessingSettingsApi/ProviderSelect";
import { BaseUrlField } from "../PostProcessingSettingsApi/BaseUrlField";
import { ApiKeyField } from "../PostProcessingSettingsApi/ApiKeyField";
import { ModelSelect } from "../PostProcessingSettingsApi/ModelSelect";
import { usePostProcessProviderState } from "../PostProcessingSettingsApi/usePostProcessProviderState";
import { useSettings } from "../../../hooks/useSettings";

const DisabledNotice: React.FC<{ children: React.ReactNode }> = ({
  children,
}) => (
  <div className="p-4 bg-mid-gray/5 rounded-lg border border-mid-gray/20">
    <p className="text-sm text-mid-gray">{children}</p>
  </div>
);

const PostProcessingSettingsApiComponent: React.FC = () => {
  const { t } = useTranslation();
  const state = usePostProcessProviderState();

  if (!state.enabled) {
    return (
      <DisabledNotice>
        {t("settings.postProcessing.disabledNotice")}
      </DisabledNotice>
    );
  }

  return (
    <>
      <SettingContainer
        title={t("settings.postProcessing.api.provider.title")}
        description={t("settings.postProcessing.api.provider.description")}
        descriptionMode="tooltip"
        layout="horizontal"
        grouped={true}
      >
        <div className="flex items-center gap-2 justify-end">
          <ProviderSelect
            options={state.providerOptions}
            value={state.selectedProviderId}
            onChange={state.handleProviderSelect}
          />
        </div>
      </SettingContainer>

      {state.isAppleProvider ? (
        <SettingContainer
          title={t("settings.postProcessing.api.appleIntelligence.title")}
          description={t(
            "settings.postProcessing.api.appleIntelligence.description",
          )}
          descriptionMode="tooltip"
          layout="stacked"
          grouped={true}
        >
          <DisabledNotice>
            {t("settings.postProcessing.api.appleIntelligence.requirements")}
          </DisabledNotice>
        </SettingContainer>
      ) : (
        <>
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
                disabled={
                  !state.selectedProvider?.allow_base_url_edit ||
                  state.isBaseUrlUpdating
                }
              />
            </div>
          </SettingContainer>

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
                providerId={state.selectedProviderId}
              />
            </div>
          </SettingContainer>
        </>
      )}

      <SettingContainer
        title={t("settings.postProcessing.api.model.title")}
        description={
          state.isAppleProvider
            ? t("settings.postProcessing.api.model.descriptionApple")
            : state.isCustomProvider
              ? t("settings.postProcessing.api.model.descriptionCustom")
              : t("settings.postProcessing.api.model.descriptionDefault")
        }
        descriptionMode="tooltip"
        layout="horizontal"
        grouped={true}
      >
        <div className="flex items-center gap-2">
          <ModelSelect
            value={state.model}
            options={state.modelOptions}
            disabled={state.isModelUpdating || state.isAppleProvider}
            isLoading={state.isFetchingModels}
            placeholder={
              state.isAppleProvider
                ? t("settings.postProcessing.api.model.placeholderApple")
                : state.modelOptions.length > 0
                  ? t(
                    "settings.postProcessing.api.model.placeholderWithOptions",
                  )
                  : t("settings.postProcessing.api.model.placeholderNoOptions")
            }
            onSelect={state.handleModelSelect}
            onCreate={state.handleModelCreate}
            onBlur={() => { }}
            onRefresh={state.handleRefreshModels}
            isRefreshing={state.isFetchingModels}
            className="flex-1 min-w-[380px]"
            providerId={state.selectedProviderId}
          />
        </div>
      </SettingContainer>
    </>
  );
};

const PostProcessingSettingsPromptsComponent: React.FC = () => {
  const { t } = useTranslation();
  const { getSetting, updateSetting, isUpdating, refreshSettings } =
    useSettings();
  const [isModalOpen, setIsModalOpen] = useState(false);
  const [editingPromptId, setEditingPromptId] = useState<string | null>(null);
  const [draftName, setDraftName] = useState("");
  const [draftText, setDraftText] = useState("");
  const [searchQuery, setSearchQuery] = useState("");

  const enabled = getSetting("post_process_enabled") || false;
  const prompts = getSetting("post_process_prompts") || [];
  const selectedPromptId = getSetting("post_process_selected_prompt_id") || "";

  // Sort prompts: active first, then alphabetically
  const sortedPrompts = [...prompts].sort((a, b) => {
    if (a.id === selectedPromptId) return -1;
    if (b.id === selectedPromptId) return 1;
    return a.name.localeCompare(b.name);
  });

  // Filter prompts by search query
  const filteredPrompts = sortedPrompts.filter(
    (prompt) =>
      prompt.name.toLowerCase().includes(searchQuery.toLowerCase()) ||
      prompt.prompt.toLowerCase().includes(searchQuery.toLowerCase())
  );

  const handleOpenCreateModal = () => {
    setEditingPromptId(null);
    setDraftName("");
    setDraftText("");
    setIsModalOpen(true);
  };

  const handleOpenEditModal = (promptId: string) => {
    const prompt = prompts.find((p) => p.id === promptId);
    if (prompt) {
      setEditingPromptId(promptId);
      setDraftName(prompt.name);
      setDraftText(prompt.prompt);
      setIsModalOpen(true);
    }
  };

  const handleCloseModal = () => {
    setIsModalOpen(false);
    setEditingPromptId(null);
    setDraftName("");
    setDraftText("");
  };

  const handleSavePrompt = async () => {
    if (!draftName.trim() || !draftText.trim()) return;

    try {
      if (editingPromptId) {
        // Update existing prompt
        await commands.updatePostProcessPrompt(
          editingPromptId,
          draftName.trim(),
          draftText.trim()
        );
      } else {
        // Create new prompt
        const result = await commands.addPostProcessPrompt(
          draftName.trim(),
          draftText.trim()
        );
        if (result.status === "ok") {
          updateSetting("post_process_selected_prompt_id", result.data.id);
        }
      }
      await refreshSettings();
      handleCloseModal();
    } catch (error) {
      console.error("Failed to save prompt:", error);
    }
  };

  const handleSetAsActive = (promptId: string) => {
    updateSetting("post_process_selected_prompt_id", promptId);
  };

  const handleDeletePrompt = async (promptId: string) => {
    if (!promptId) return;

    try {
      await commands.deletePostProcessPrompt(promptId);
      await refreshSettings();
    } catch (error) {
      console.error("Failed to delete prompt:", error);
    }
  };

  if (!enabled) {
    return (
      <DisabledNotice>
        {t("settings.postProcessing.disabledNotice")}
      </DisabledNotice>
    );
  }

  return (
    <div className="px-4 py-2">
      {/* Header with Search and Create Button */}
      <div className="flex items-center gap-3 mb-4">
        {/* Search Bar */}
        <div className="flex-1 relative">
          <svg
            className="absolute left-3 top-1/2 transform -translate-y-1/2 w-4 h-4 text-mid-gray"
            fill="none"
            stroke="currentColor"
            viewBox="0 0 24 24"
          >
            <path
              strokeLinecap="round"
              strokeLinejoin="round"
              strokeWidth={2}
              d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z"
            />
          </svg>
          <Input
            type="text"
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            placeholder={t("settings.postProcessing.prompts.searchPlaceholder")}
            className="w-full pl-9"
            variant="compact"
          />
        </div>

        {/* Create New Prompt Button */}
        <Button
          onClick={handleOpenCreateModal}
          variant="primary"
          size="md"
          className="flex items-center gap-1 whitespace-nowrap"
        >
          <svg
            className="w-4 h-4"
            fill="none"
            stroke="currentColor"
            viewBox="0 0 24 24"
          >
            <path
              strokeLinecap="round"
              strokeLinejoin="round"
              strokeWidth={2}
              d="M12 4v16m8-8H4"
            />
          </svg>
          {t("settings.postProcessing.prompts.createNew")}
        </Button>
      </div>

      {/* Prompt Cards List */}
      <div className="space-y-2">
        {filteredPrompts.length === 0 ? (
          <div className="p-4 bg-mid-gray/5 rounded-lg border border-mid-gray/20 text-center">
            <p className="text-sm text-mid-gray">
              {prompts.length === 0
                ? t("settings.postProcessing.prompts.createFirst")
                : t("settings.postProcessing.prompts.noSearchResults")}
            </p>
          </div>
        ) : (
          filteredPrompts.map((prompt) => {
            const isActive = prompt.id === selectedPromptId;
            return (
              <div
                key={prompt.id}
                className={`p-3 rounded-lg border transition-all duration-150 ${isActive
                  ? "border-logo-primary/50 bg-logo-primary/5"
                  : "border-mid-gray/20 bg-mid-gray/5 hover:border-mid-gray/40"
                  }`}
              >
                <div className="flex items-start justify-between gap-3">
                  {/* Prompt Content */}
                  <div className="flex-1 min-w-0">
                    <div className="flex items-center gap-2 mb-1">
                      <h4 className="text-sm font-semibold truncate">
                        {prompt.name}
                      </h4>
                      {isActive && (
                        <span className="px-1.5 py-0.5 text-xs font-medium bg-logo-primary/20 text-logo-primary rounded">
                          {t("settings.postProcessing.prompts.active")}
                        </span>
                      )}
                    </div>
                    <p className="text-xs text-mid-gray line-clamp-2">
                      {prompt.prompt}
                    </p>
                  </div>

                  {/* Action Buttons */}
                  <div className="flex items-center gap-1 flex-shrink-0">
                    {/* Edit Button */}
                    <button
                      onClick={() => handleOpenEditModal(prompt.id)}
                      className="p-1.5 rounded-lg hover:bg-mid-gray/20 transition-colors duration-150 cursor-pointer"
                      title={t("settings.postProcessing.prompts.edit")}
                    >
                      <svg
                        className="w-4 h-4 text-mid-gray hover:text-text"
                        fill="none"
                        stroke="currentColor"
                        viewBox="0 0 24 24"
                      >
                        <path
                          strokeLinecap="round"
                          strokeLinejoin="round"
                          strokeWidth={2}
                          d="M11 5H6a2 2 0 00-2 2v11a2 2 0 002 2h11a2 2 0 002-2v-5m-1.414-9.414a2 2 0 112.828 2.828L11.828 15H9v-2.828l8.586-8.586z"
                        />
                      </svg>
                    </button>

                    {/* Set as Active Button */}
                    <button
                      onClick={() => handleSetAsActive(prompt.id)}
                      disabled={
                        isActive || isUpdating("post_process_selected_prompt_id")
                      }
                      className={`p-1.5 rounded-lg transition-colors duration-150 cursor-pointer ${isActive
                        ? "text-logo-primary"
                        : "hover:bg-mid-gray/20 text-mid-gray hover:text-logo-primary"
                        } disabled:opacity-50 disabled:cursor-not-allowed`}
                      title={
                        isActive
                          ? t("settings.postProcessing.prompts.currentlyActive")
                          : t("settings.postProcessing.prompts.setAsActive")
                      }
                    >
                      <svg
                        className="w-4 h-4"
                        fill={isActive ? "currentColor" : "none"}
                        stroke="currentColor"
                        viewBox="0 0 24 24"
                      >
                        <path
                          strokeLinecap="round"
                          strokeLinejoin="round"
                          strokeWidth={2}
                          d="M11.049 2.927c.3-.921 1.603-.921 1.902 0l1.519 4.674a1 1 0 00.95.69h4.915c.969 0 1.371 1.24.588 1.81l-3.976 2.888a1 1 0 00-.363 1.118l1.518 4.674c.3.922-.755 1.688-1.538 1.118l-3.976-2.888a1 1 0 00-1.176 0l-3.976 2.888c-.783.57-1.838-.197-1.538-1.118l1.518-4.674a1 1 0 00-.363-1.118l-3.976-2.888c-.784-.57-.38-1.81.588-1.81h4.914a1 1 0 00.951-.69l1.519-4.674z"
                        />
                      </svg>
                    </button>

                    {/* Delete Button */}
                    <button
                      onClick={() => handleDeletePrompt(prompt.id)}
                      disabled={isActive}
                      className="p-1.5 rounded-lg hover:bg-red-500/20 transition-colors duration-150 cursor-pointer text-mid-gray hover:text-red-500 disabled:opacity-50 disabled:cursor-not-allowed"
                      title={t("settings.postProcessing.prompts.deletePrompt")}
                    >
                      <svg
                        className="w-4 h-4"
                        fill="none"
                        stroke="currentColor"
                        viewBox="0 0 24 24"
                      >
                        <path
                          strokeLinecap="round"
                          strokeLinejoin="round"
                          strokeWidth={2}
                          d="M19 7l-.867 12.142A2 2 0 0116.138 21H7.862a2 2 0 01-1.995-1.858L5 7m5 4v6m4-6v6m1-10V4a1 1 0 00-1-1h-4a1 1 0 00-1 1v3M4 7h16"
                        />
                      </svg>
                    </button>
                  </div>
                </div>
              </div>
            );
          })
        )}
      </div>

      {/* Create/Edit Modal */}
      <Modal
        isOpen={isModalOpen}
        onClose={handleCloseModal}
        title={
          editingPromptId
            ? t("settings.postProcessing.prompts.editPrompt")
            : t("settings.postProcessing.prompts.createNewPrompt")
        }
      >
        <div className="space-y-4">
          <div className="space-y-2">
            <label className="text-sm font-semibold">
              {t("settings.postProcessing.prompts.promptLabel")}
            </label>
            <Input
              type="text"
              value={draftName}
              onChange={(e) => setDraftName(e.target.value)}
              placeholder={t(
                "settings.postProcessing.prompts.promptLabelPlaceholder"
              )}
              variant="compact"
              className="w-full"
            />
          </div>

          <div className="space-y-2">
            <label className="text-sm font-semibold">
              {t("settings.postProcessing.prompts.promptInstructions")}
            </label>
            <Textarea
              value={draftText}
              onChange={(e) => setDraftText(e.target.value)}
              placeholder={t(
                "settings.postProcessing.prompts.promptInstructionsPlaceholder"
              )}
              className="w-full"
            />
            <p
              className="text-xs text-mid-gray/70"
              dangerouslySetInnerHTML={{
                __html: t("settings.postProcessing.prompts.promptTip"),
              }}
            />
          </div>

          <div className="flex gap-2 justify-end pt-2">
            <Button onClick={handleCloseModal} variant="secondary" size="md">
              {t("settings.postProcessing.prompts.cancel")}
            </Button>
            <Button
              onClick={handleSavePrompt}
              variant="primary"
              size="md"
              disabled={!draftName.trim() || !draftText.trim()}
            >
              {editingPromptId
                ? t("settings.postProcessing.prompts.updatePrompt")
                : t("settings.postProcessing.prompts.createPrompt")}
            </Button>
          </div>
        </div>
      </Modal>
    </div>
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
      <SettingsGroup title={t("settings.postProcessing.api.title")}>
        <PostProcessingSettingsApi />
      </SettingsGroup>

      <SettingsGroup title={t("settings.postProcessing.prompts.title")}>
        <PostProcessingSettingsPrompts />
      </SettingsGroup>
    </div>
  );
};

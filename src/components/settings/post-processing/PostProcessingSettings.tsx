import React, { useEffect, useState } from "react";
import { Trans, useTranslation } from "react-i18next";
import { Plus, RefreshCcw, X } from "lucide-react";
import { ask } from "@tauri-apps/plugin-dialog";
import { toast } from "sonner";
import { commands, type PostProcessProfile } from "@/bindings";

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
import {
  DEFAULT_PROFILE_ID,
  MAX_PROFILE_NAME_LENGTH,
  getProfileDisplayName,
} from "../../../lib/utils/postProcessProfile";

interface ProfileProps {
  profileId?: string;
}

const PostProcessingSettingsApiComponent: React.FC<ProfileProps> = ({
  profileId = DEFAULT_PROFILE_ID,
}) => {
  const { t } = useTranslation();
  const state = usePostProcessProviderState(profileId);

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

const PostProcessingSettingsPromptsComponent: React.FC<ProfileProps> = ({
  profileId = DEFAULT_PROFILE_ID,
}) => {
  const { t } = useTranslation();
  const { getSetting, refreshSettings } = useSettings();
  const [isCreating, setIsCreating] = useState(false);
  const [isSelecting, setIsSelecting] = useState(false);
  const [draftName, setDraftName] = useState("");
  const [draftText, setDraftText] = useState("");

  const profile = getSetting("post_process_profiles")?.find(
    (p) => p.id === profileId,
  );
  const prompts = profile?.prompts || [];
  const selectedPromptId = profile?.selected_prompt_id || "";
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

  const selectPrompt = async (promptId: string) => {
    setIsSelecting(true);
    try {
      const result = await commands.setPostProcessSelectedPrompt(
        profileId,
        promptId,
      );
      if (result.status === "error") {
        console.error("Failed to select prompt:", result.error);
      }
      await refreshSettings();
    } catch (error) {
      console.error("Failed to select prompt:", error);
    } finally {
      setIsSelecting(false);
    }
  };

  const handlePromptSelect = (promptId: string | null) => {
    if (!promptId) return;
    void selectPrompt(promptId);
    setIsCreating(false);
  };

  const handleCreatePrompt = async () => {
    if (!draftName.trim() || !draftText.trim()) return;

    try {
      const result = await commands.addPostProcessPrompt(
        profileId,
        draftName.trim(),
        draftText.trim(),
      );
      if (result.status === "ok") {
        await selectPrompt(result.data.id);
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
        profileId,
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
      await commands.deletePostProcessPrompt(profileId, promptId);
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
            disabled={isSelecting || isCreating}
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
              <p className="text-xs text-mid-gray/70">
                <Trans
                  i18nKey="settings.postProcessing.prompts.promptTip"
                  components={{ code: <code /> }}
                />
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
              <p className="text-xs text-mid-gray/70">
                <Trans
                  i18nKey="settings.postProcessing.prompts.promptTip"
                  components={{ code: <code /> }}
                />
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

export const PostProcessingSettingsPrompts = React.memo(
  PostProcessingSettingsPromptsComponent,
);
PostProcessingSettingsPrompts.displayName = "PostProcessingSettingsPrompts";

interface ProfileTabsProps {
  profiles: PostProcessProfile[];
  activeId: string;
  onSelect: (profileId: string) => void;
  onAdd: () => void;
  onRename: (profile: PostProcessProfile, name: string) => Promise<void>;
  onDelete: (profile: PostProcessProfile) => void;
}

const ProfileTabs: React.FC<ProfileTabsProps> = ({
  profiles,
  activeId,
  onSelect,
  onAdd,
  onRename,
  onDelete,
}) => {
  const { t } = useTranslation();
  const [editingId, setEditingId] = useState<string | null>(null);
  const [draftName, setDraftName] = useState("");

  const startRename = (profile: PostProcessProfile) => {
    setEditingId(profile.id);
    setDraftName(getProfileDisplayName(profile, t));
  };

  const commitRename = async (profile: PostProcessProfile) => {
    if (editingId !== profile.id) return;
    setEditingId(null);
    const name = draftName.trim();
    // An empty name is only meaningful for Default (back to its localized
    // label); for other tabs it just cancels.
    if (!name && profile.id !== DEFAULT_PROFILE_ID) return;
    if (name === getProfileDisplayName(profile, t)) return;
    await onRename(profile, name);
  };

  return (
    <div
      role="tablist"
      aria-label={t("settings.postProcessing.profiles.title")}
      className="flex flex-wrap items-center gap-1.5"
    >
      {profiles.map((profile) => {
        const isActive = profile.id === activeId;
        const displayName = getProfileDisplayName(profile, t);
        return (
          <div
            key={profile.id}
            className={`flex items-center h-8 text-sm font-medium rounded-lg transition-colors ${
              isActive
                ? "bg-logo-primary/20 text-logo-primary hover:bg-logo-primary/30"
                : "bg-mid-gray/10 text-text/60 hover:bg-mid-gray/20"
            }`}
          >
            {editingId === profile.id ? (
              <input
                autoFocus
                value={draftName}
                maxLength={MAX_PROFILE_NAME_LENGTH}
                aria-label={t("settings.postProcessing.profiles.nameLabel")}
                onChange={(e) => setDraftName(e.target.value)}
                onFocus={(e) => e.target.select()}
                onBlur={() => void commitRename(profile)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") {
                    e.currentTarget.blur();
                  } else if (e.key === "Escape") {
                    setEditingId(null);
                  }
                }}
                className="h-6 w-32 mx-1 px-2 rounded-md bg-background border border-logo-primary text-text text-sm focus:outline-none"
              />
            ) : (
              <button
                type="button"
                role="tab"
                aria-selected={isActive}
                onClick={() => onSelect(profile.id)}
                onDoubleClick={() => startRename(profile)}
                title={t("settings.postProcessing.profiles.renameHint")}
                className={`h-full cursor-pointer ${
                  profile.id === DEFAULT_PROFILE_ID ? "px-3" : "ps-3 pe-1"
                }`}
              >
                {displayName}
              </button>
            )}
            {profile.id !== DEFAULT_PROFILE_ID && editingId !== profile.id && (
              <button
                type="button"
                onClick={() => onDelete(profile)}
                title={t("settings.postProcessing.profiles.delete", {
                  name: displayName,
                })}
                aria-label={t("settings.postProcessing.profiles.delete", {
                  name: displayName,
                })}
                className="flex items-center justify-center h-6 w-6 me-1 rounded-md cursor-pointer hover:bg-mid-gray/20"
              >
                <X className="w-3.5 h-3.5" />
              </button>
            )}
          </div>
        );
      })}
      <button
        type="button"
        onClick={onAdd}
        title={t("settings.postProcessing.profiles.add")}
        aria-label={t("settings.postProcessing.profiles.add")}
        className="flex items-center justify-center w-8 h-8 rounded-lg cursor-pointer transition-colors bg-mid-gray/10 text-text/60 hover:bg-mid-gray/20"
      >
        <Plus className="w-3.5 h-3.5" />
      </button>
    </div>
  );
};

export const PostProcessingSettings: React.FC = () => {
  const { t } = useTranslation();
  const { getSetting, refreshSettings } = useSettings();
  const [activeId, setActiveId] = useState(DEFAULT_PROFILE_ID);

  const profiles = getSetting("post_process_profiles") || [];
  const activeProfile =
    profiles.find((profile) => profile.id === activeId) || profiles[0];

  const handleAddProfile = async () => {
    // New tabs are named "<localized Default> N".
    const result = await commands.addPostProcessProfile(
      t("settings.postProcessing.profiles.defaultName"),
    );
    if (result.status === "ok") {
      await refreshSettings();
      setActiveId(result.data.id);
    } else {
      console.error("Failed to add profile:", result.error);
      toast.error(t("settings.postProcessing.profiles.errors.add"));
    }
  };

  const handleRenameProfile = async (
    profile: PostProcessProfile,
    name: string,
  ) => {
    const result = await commands.renamePostProcessProfile(profile.id, name);
    if (result.status === "ok") {
      await refreshSettings();
    } else {
      console.error("Failed to rename profile:", result.error);
      toast.error(t("settings.postProcessing.profiles.errors.rename"));
    }
  };

  const handleDeleteProfile = async (profile: PostProcessProfile) => {
    const confirmed = await ask(
      t("settings.postProcessing.profiles.deleteConfirm", {
        name: getProfileDisplayName(profile, t),
      }),
      {
        title: t("settings.postProcessing.profiles.deleteTitle"),
        kind: "warning",
      },
    );
    if (!confirmed) return;

    const result = await commands.deletePostProcessProfile(profile.id);
    if (result.status === "ok") {
      if (activeId === profile.id) {
        setActiveId(DEFAULT_PROFILE_ID);
      }
      await refreshSettings();
    } else {
      console.error("Failed to delete profile:", result.error);
      toast.error(t("settings.postProcessing.profiles.errors.delete"));
    }
  };

  return (
    <div className="max-w-3xl w-full mx-auto space-y-6">
      <ProfileTabs
        profiles={profiles}
        activeId={activeProfile?.id ?? DEFAULT_PROFILE_ID}
        onSelect={setActiveId}
        onAdd={handleAddProfile}
        onRename={handleRenameProfile}
        onDelete={handleDeleteProfile}
      />

      {activeProfile && (
        <>
          <SettingsGroup title={t("settings.postProcessing.hotkey.title")}>
            <ShortcutInput
              shortcutId={activeProfile.binding_id}
              title={t("settings.postProcessing.profiles.shortcutTitle", {
                name: getProfileDisplayName(activeProfile, t),
              })}
              description={t(
                "settings.general.shortcut.bindings.transcribe_with_post_process.description",
              )}
              descriptionMode="tooltip"
              grouped={true}
            />
          </SettingsGroup>

          <SettingsGroup title={t("settings.postProcessing.api.title")}>
            <PostProcessingSettingsApi
              key={activeProfile.id}
              profileId={activeProfile.id}
            />
          </SettingsGroup>

          <SettingsGroup title={t("settings.postProcessing.prompts.title")}>
            <PostProcessingSettingsPrompts
              key={activeProfile.id}
              profileId={activeProfile.id}
            />
          </SettingsGroup>
        </>
      )}
    </div>
  );
};

import React, { useState } from "react";
import { useTranslation } from "react-i18next";
import { commands } from "@/bindings";

import {
  Dropdown,
  SettingContainer,
  SettingsGroup,
  Textarea,
} from "@/components/ui";
import { Button } from "../../ui/Button";
import { Input } from "../../ui/Input";
import { useSettings } from "../../../hooks/useSettings";

const PostProcessingActionsComponent: React.FC = () => {
  const { t } = useTranslation();
  const { getSetting, refreshSettings } = useSettings();
  const [editingAction, setEditingAction] = useState<{
    key: number;
    originalKey?: number;
    name: string;
    prompt: string;
    savedModelId: string;
    isNew: boolean;
  } | null>(null);

  const actions = getSetting("post_process_actions") || [];
  const savedModels = getSetting("saved_processing_models") || [];

  const modelDropdownOptions = [
    {
      value: "__default__",
      label: t("settings.postProcessing.actions.defaultModel"),
    },
    ...savedModels.map((m) => ({
      value: m.id,
      label: m.label,
    })),
  ];

  const usedKeys = new Set(actions.map((a) => a.key));
  const nextAvailableKey = Array.from({ length: 9 }, (_, i) => i + 1).find(
    (k) => !usedKeys.has(k),
  );

  const availableKeysForEditing = Array.from({ length: 9 }, (_, i) => i + 1)
    .filter(
      (k) =>
        !usedKeys.has(k) ||
        k === editingAction?.key ||
        k === editingAction?.originalKey,
    )
    .map((k) => ({ value: String(k), label: String(k) }));

  const handleStartCreate = () => {
    if (!nextAvailableKey) return;
    setEditingAction({
      key: nextAvailableKey,
      name: "",
      prompt: "",
      savedModelId: "",
      isNew: true,
    });
  };

  const handleStartEdit = (action: {
    key: number;
    name: string;
    prompt: string;
    model?: string | null;
    provider_id?: string | null;
  }) => {
    let savedModelId = "";
    if (action.provider_id && action.model) {
      const id = `${action.provider_id}:${action.model}`;
      if (savedModels.some((m) => m.id === id)) {
        savedModelId = id;
      }
    }
    setEditingAction({
      key: action.key,
      originalKey: action.key,
      name: action.name,
      prompt: action.prompt,
      savedModelId,
      isNew: false,
    });
  };

  const handleSave = async () => {
    if (
      !editingAction ||
      !editingAction.name.trim() ||
      !editingAction.prompt.trim()
    )
      return;

    try {
      let model: string | null = null;
      let providerId: string | null = null;
      if (editingAction.savedModelId) {
        const saved = savedModels.find(
          (m) => m.id === editingAction.savedModelId,
        );
        if (saved) {
          model = saved.model_id;
          providerId = saved.provider_id;
        }
      }
      if (editingAction.isNew) {
        await commands.addPostProcessAction(
          editingAction.key,
          editingAction.name.trim(),
          editingAction.prompt.trim(),
          model,
          providerId,
        );
      } else if (
        editingAction.originalKey !== undefined &&
        editingAction.originalKey !== editingAction.key
      ) {
        await commands.deletePostProcessAction(editingAction.originalKey);
        await commands.addPostProcessAction(
          editingAction.key,
          editingAction.name.trim(),
          editingAction.prompt.trim(),
          model,
          providerId,
        );
      } else {
        await commands.updatePostProcessAction(
          editingAction.key,
          editingAction.name.trim(),
          editingAction.prompt.trim(),
          model,
          providerId,
        );
      }
      await refreshSettings();
      setEditingAction(null);
    } catch (error) {
      console.error("Failed to save action:", error);
    }
  };

  const handleDelete = async (key: number) => {
    try {
      await commands.deletePostProcessAction(key);
      await refreshSettings();
      if (editingAction?.key === key) {
        setEditingAction(null);
      }
    } catch (error) {
      console.error("Failed to delete action:", error);
    }
  };

  return (
    <SettingContainer
      title={t("settings.postProcessing.actions.title")}
      description={t("settings.postProcessing.actions.description")}
      descriptionMode="tooltip"
      layout="stacked"
      grouped={true}
    >
      <div className="space-y-3">
        {actions.length > 0 && (
          <div className="space-y-1">
            {[...actions]
              .sort((a, b) => a.key - b.key)
              .map((action) => (
                <div
                  key={action.key}
                  className="flex items-center gap-3 p-2 rounded-md hover:bg-mid-gray/5 cursor-pointer group"
                  onClick={() => handleStartEdit(action)}
                >
                  <span className="flex items-center justify-center w-6 h-6 rounded bg-blue-500/15 text-blue-400 text-xs font-bold font-mono flex-shrink-0">
                    {action.key}
                  </span>
                  <span className="text-sm text-text flex-1 truncate">
                    {action.name}
                    {action.provider_id && action.model && (
                      <span className="text-xs text-mid-gray/60 ml-2">
                        {savedModels.find(
                          (m) =>
                            m.id === `${action.provider_id}:${action.model}`,
                        )?.label || action.model}
                      </span>
                    )}
                  </span>
                  <button
                    className="text-xs text-mid-gray/60 hover:text-red-400 opacity-0 group-hover:opacity-100 transition-opacity px-1"
                    onClick={(e) => {
                      e.stopPropagation();
                      handleDelete(action.key);
                    }}
                  >
                    {t("settings.postProcessing.actions.delete")}
                  </button>
                </div>
              ))}
          </div>
        )}

        {actions.length === 0 && !editingAction && (
          <div className="p-3 bg-mid-gray/5 rounded-md border border-mid-gray/20">
            <p className="text-sm text-mid-gray">
              {t("settings.postProcessing.actions.createFirst")}
            </p>
          </div>
        )}

        {editingAction && (
          <div className="space-y-3 p-3 rounded-md border border-mid-gray/20 bg-mid-gray/5">
            <div className="flex gap-3">
              <div className="space-y-1 flex flex-col">
                <label className="text-sm font-semibold">
                  {t("settings.postProcessing.actions.key")}
                </label>
                <select
                  value={editingAction.key}
                  onChange={(e) =>
                    setEditingAction({
                      ...editingAction,
                      key: Number(e.target.value),
                    })
                  }
                  className="w-10 h-8 rounded bg-blue-500/15 text-blue-400 text-sm font-bold font-mono text-center appearance-none cursor-pointer border border-transparent hover:border-blue-400/40 transition-colors"
                >
                  {availableKeysForEditing.map((opt) => (
                    <option key={opt.value} value={opt.value}>
                      {opt.label}
                    </option>
                  ))}
                </select>
              </div>
              <div className="space-y-1 flex flex-col flex-1">
                <label className="text-sm font-semibold">
                  {t("settings.postProcessing.actions.name")}
                </label>
                <Input
                  type="text"
                  value={editingAction.name}
                  onChange={(e) =>
                    setEditingAction({ ...editingAction, name: e.target.value })
                  }
                  placeholder={t(
                    "settings.postProcessing.actions.namePlaceholder",
                  )}
                  variant="compact"
                />
              </div>
            </div>

            <div className="space-y-1 flex flex-col">
              <label className="text-sm font-semibold">
                {t("settings.postProcessing.actions.prompt")}
              </label>
              <Textarea
                value={editingAction.prompt}
                onChange={(e) =>
                  setEditingAction({ ...editingAction, prompt: e.target.value })
                }
                placeholder={t(
                  "settings.postProcessing.actions.promptPlaceholder",
                )}
              />
              <p
                className="text-xs text-mid-gray/70"
                dangerouslySetInnerHTML={{
                  __html: t("settings.postProcessing.actions.promptTip"),
                }}
              />
            </div>

            <div className="space-y-1 flex flex-col">
              <label className="text-sm font-semibold">
                {t("settings.postProcessing.actions.model")}
              </label>
              <Dropdown
                selectedValue={editingAction.savedModelId || null}
                options={modelDropdownOptions}
                onSelect={(value) =>
                  setEditingAction({
                    ...editingAction,
                    savedModelId: value === "__default__" ? "" : value,
                  })
                }
                placeholder={t(
                  "settings.postProcessing.actions.modelPlaceholder",
                )}
              />
              <p className="text-xs text-mid-gray/70">
                {t("settings.postProcessing.actions.modelTip")}
              </p>
            </div>

            <div className="flex gap-2 pt-1">
              <Button
                onClick={handleSave}
                variant="primary"
                size="md"
                disabled={
                  !editingAction.name.trim() || !editingAction.prompt.trim()
                }
              >
                {t("settings.postProcessing.actions.save")}
              </Button>
              <Button
                onClick={() => setEditingAction(null)}
                variant="secondary"
                size="md"
              >
                {t("settings.postProcessing.actions.cancel")}
              </Button>
              {!editingAction.isNew && (
                <Button
                  onClick={() =>
                    handleDelete(editingAction.originalKey ?? editingAction.key)
                  }
                  variant="secondary"
                  size="md"
                >
                  {t("settings.postProcessing.actions.delete")}
                </Button>
              )}
            </div>
          </div>
        )}

        {!editingAction && actions.length < 9 && (
          <Button onClick={handleStartCreate} variant="primary" size="md">
            {t("settings.postProcessing.actions.addAction")}
          </Button>
        )}

        {actions.length >= 9 && !editingAction && (
          <p className="text-xs text-mid-gray/60">
            {t("settings.postProcessing.actions.maxActionsReached")}
          </p>
        )}
      </div>
    </SettingContainer>
  );
};

const PostProcessingActions = React.memo(PostProcessingActionsComponent);
PostProcessingActions.displayName = "PostProcessingActions";

export const PostProcessingSettings: React.FC = () => {
  const { t } = useTranslation();

  return (
    <div className="max-w-3xl w-full mx-auto space-y-6">
      <SettingsGroup title={t("settings.postProcessing.actions.title")}>
        <PostProcessingActions />
      </SettingsGroup>
    </div>
  );
};

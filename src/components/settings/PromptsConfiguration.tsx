import React, { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useSettings } from "../../hooks/useSettings";
import { Input } from "../ui/Input";
import { Button } from "../ui/Button";
import { SettingContainer } from "../ui/SettingContainer";
import { Dropdown } from "../ui/Dropdown";
import { Textarea } from "../ui/Textarea";
import type { LLMPrompt } from "../../lib/types";

export const PromptsConfiguration: React.FC = React.memo(() => {
  const { getSetting, updateSetting, isUpdating, refreshSettings } = useSettings();
  const [isCreating, setIsCreating] = useState(false);
  const [isEditing, setIsEditing] = useState(false);
  const [editingPromptId, setEditingPromptId] = useState<string | null>(null);
  const [promptName, setPromptName] = useState("");
  const [promptText, setPromptText] = useState("");

  const enabled = getSetting("post_process_enabled") || false;
  const prompts = getSetting("post_process_prompts") || [];
  const selectedPromptId = getSetting("post_process_selected_prompt_id") || "";

  const handlePromptSelect = (promptId: string) => {
    updateSetting("post_process_selected_prompt_id", promptId);
  };

  const handleCreatePrompt = async () => {
    if (!promptName.trim() || !promptText.trim()) return;

    try {
      const newPrompt = await invoke<LLMPrompt>("add_post_process_prompt", {
        name: promptName.trim(),
        prompt: promptText.trim(),
      });
      await refreshSettings();
      // Automatically select the newly created prompt
      updateSetting("post_process_selected_prompt_id", newPrompt.id);
      setPromptName("");
      setPromptText("");
      setIsCreating(false);
    } catch (error) {
      console.error("Failed to create prompt:", error);
    }
  };

  const handleEditPrompt = (prompt: LLMPrompt) => {
    setEditingPromptId(prompt.id);
    setPromptName(prompt.name);
    setPromptText(prompt.prompt);
    setIsEditing(true);
    setIsCreating(false);
  };

  const handleUpdatePrompt = async () => {
    if (!editingPromptId || !promptName.trim() || !promptText.trim()) return;

    try {
      await invoke("update_post_process_prompt", {
        id: editingPromptId,
        name: promptName.trim(),
        prompt: promptText.trim(),
      });
      await refreshSettings();
      setPromptName("");
      setPromptText("");
      setIsEditing(false);
      setEditingPromptId(null);
    } catch (error) {
      console.error("Failed to update prompt:", error);
    }
  };

  const handleDeletePrompt = async (promptId: string) => {
    try {
      await invoke("delete_post_process_prompt", { id: promptId });
      await refreshSettings();
    } catch (error) {
      console.error("Failed to delete prompt:", error);
    }
  };

  const handleCancelEdit = () => {
    setIsCreating(false);
    setIsEditing(false);
    setEditingPromptId(null);
    setPromptName("");
    setPromptText("");
  };

  if (!enabled) {
    return (
      <div className="p-4 bg-mid-gray/5 rounded-lg border border-mid-gray/20 text-center">
        <p className="text-sm text-mid-gray">
          Post processing is currently disabled. Enable it in Debug settings to configure.
        </p>
      </div>
    );
  }

  return (
    <SettingContainer
      title="Prompt"
      description="Select or create prompts for text processing. Use ${output} in your prompt to reference the transcribed text."
      descriptionMode="tooltip"
      layout="stacked"
      grouped={true}
    >
      <div className="space-y-3">
        <div className="flex items-center gap-2">
          <Dropdown
            options={prompts.map((p) => ({ value: p.id, label: p.name }))}
            selectedValue={selectedPromptId}
            onSelect={handlePromptSelect}
            disabled={isUpdating("post_process_selected_prompt_id") || isCreating || isEditing || prompts.length === 0}
            placeholder={prompts.length === 0 ? "No prompts available" : "Select a prompt"}
            className="flex-grow"
          />
          <Button
            onClick={() => {
              const selected = prompts.find((p) => p.id === selectedPromptId);
              if (selected) handleEditPrompt(selected);
            }}
            variant="secondary"
            size="md"
            disabled={!selectedPromptId || isCreating || isEditing}
          >
            Edit
          </Button>
          <Button
            onClick={() => handleDeletePrompt(selectedPromptId)}
            variant="secondary"
            size="md"
            disabled={!selectedPromptId || isCreating || isEditing}
          >
            Delete
          </Button>
          <Button
            onClick={() => {
              setIsCreating(true);
              setIsEditing(false);
              setPromptName("");
              setPromptText("");
            }}
            variant="primary"
            size="md"
            disabled={isCreating || isEditing}
          >
            New
          </Button>
        </div>

        {(isCreating || isEditing) && (
          <div className="space-y-3">
            <div className="space-y-2 block flex flex-col">
              <label className="text-sm font-semibold text-mid-gray">
                Prompt Name
              </label>
              <Input
                type="text"
                value={promptName}
                onChange={(e) => setPromptName(e.target.value)}
                placeholder="Enter prompt name"
                variant="compact"
              />
            </div>
            
            <div className="space-y-2 block flex flex-col">
              <label className="text-sm font-semibold text-mid-gray">
                Prompt Text
              </label>
              <Textarea
                value={promptText}
                onChange={(e) => setPromptText(e.target.value)}
                placeholder="Enter your prompt here. Use ${output} to reference the transcribed text.&#10;&#10;Example: Improve the following text: ${output}"
              />
              <p className="text-xs text-mid-gray/70">
                Tip: Use <code className="px-1 py-0.5 bg-mid-gray/20 rounded text-xs">$&#123;output&#125;</code> to insert the transcribed text in your prompt.
              </p>
            </div>

            <div className="flex gap-2 pt-2">
              <Button
                onClick={isEditing ? handleUpdatePrompt : handleCreatePrompt}
                variant="primary"
                size="md"
                disabled={!promptName.trim() || !promptText.trim()}
              >
                {isEditing ? "Update Prompt" : "Create Prompt"}
              </Button>
              <Button
                onClick={handleCancelEdit}
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
});

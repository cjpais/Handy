import React, { useEffect, useState, useRef } from "react";
import { type } from "@tauri-apps/plugin-os";
import {
  getKeyName,
  formatKeyCombination,
  normalizeKey,
  type OSType,
} from "../../lib/utils/keyboard";
import { ResetButton } from "../ui/ResetButton";
import { SettingContainer } from "../ui/SettingContainer";
import { useSettings } from "../../hooks/useSettings";
import { invoke } from "@tauri-apps/api/core";
import { toast } from "sonner";

interface PostProcessingHotkeyProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const PostProcessingHotkey: React.FC<PostProcessingHotkeyProps> = ({
  descriptionMode = "tooltip",
  grouped = false,
}) => {
  const { getSetting, updateBinding, resetBinding, isUpdating, isLoading } =
    useSettings();
  const [keyPressed, setKeyPressed] = useState<string[]>([]);
  const [recordedKeys, setRecordedKeys] = useState<string[]>([]);
  const [editingShortcutId, setEditingShortcutId] = useState<string | null>(
    null,
  );
  const [originalBinding, setOriginalBinding] = useState<string>("");
  const [osType, setOsType] = useState<OSType>("unknown");
  const shortcutRef = useRef<HTMLDivElement | null>(null);

  const bindings = getSetting("bindings") || {};
  const bindingId = "transcribe_with_post_process";

  // Create a default binding structure if it doesn't exist yet (for existing installations)
  const binding = bindings[bindingId] || {
    id: bindingId,
    name: "Transcribe with Post-Processing",
    description:
      "Converts your speech into text and applies AI post-processing.",
    default_binding: "",
    current_binding: "",
  };

  const enabled = getSetting("post_process_enabled") || false;

  useEffect(() => {
    const detectOsType = async () => {
      try {
        const detectedType = type();
        let normalizedType: OSType;

        switch (detectedType) {
          case "macos":
            normalizedType = "macos";
            break;
          case "windows":
            normalizedType = "windows";
            break;
          case "linux":
            normalizedType = "linux";
            break;
          default:
            normalizedType = "unknown";
        }

        setOsType(normalizedType);
      } catch (error) {
        console.error("Error detecting OS type:", error);
        setOsType("unknown");
      }
    };

    detectOsType();
  }, []);

  useEffect(() => {
    if (editingShortcutId === null) return;

    let cleanup = false;

    const handleKeyDown = async (e: KeyboardEvent) => {
      if (cleanup) return;
      if (e.repeat) return;
      if (e.key === "Escape") {
        if (editingShortcutId && originalBinding) {
          try {
            await updateBinding(editingShortcutId, originalBinding);
            await invoke("resume_binding", { id: editingShortcutId }).catch(
              console.error,
            );
          } catch (error) {
            console.error("Failed to restore original binding:", error);
            toast.error("Failed to restore original shortcut");
          }
        } else if (editingShortcutId) {
          await invoke("resume_binding", { id: editingShortcutId }).catch(
            console.error,
          );
        }
        setEditingShortcutId(null);
        setKeyPressed([]);
        setRecordedKeys([]);
        setOriginalBinding("");
        return;
      }

      e.preventDefault();

      const keyName = getKeyName(e, osType);
      const isModifier = ["alt", "ctrl", "meta", "shift"].includes(
        keyName.toLowerCase(),
      );

      setKeyPressed((prev) => {
        const normalized = normalizeKey(keyName);
        if (!prev.includes(normalized)) {
          return [...prev, normalized];
        }
        return prev;
      });

      if (!isModifier) {
        const finalKeys = Array.from(
          new Set([...keyPressed, normalizeKey(keyName)]),
        );
        setRecordedKeys(finalKeys);
      }
    };

    const handleKeyUp = async (e: KeyboardEvent) => {
      if (cleanup) return;

      const keyName = getKeyName(e, osType);
      setKeyPressed((prev) => prev.filter((k) => k !== normalizeKey(keyName)));

      if (recordedKeys.length > 0 && keyPressed.length <= 1) {
        const shortcutString = recordedKeys.join("+");

        try {
          await updateBinding(bindingId, shortcutString);
          await invoke("resume_binding", { id: bindingId }).catch(
            console.error,
          );
        } catch (error) {
          console.error("Failed to set new binding:", error);
          toast.error("Failed to set new shortcut");

          if (originalBinding) {
            try {
              await updateBinding(bindingId, originalBinding);
            } catch (restoreError) {
              console.error(
                "Failed to restore original binding:",
                restoreError,
              );
            }
          }
        }

        setEditingShortcutId(null);
        setKeyPressed([]);
        setRecordedKeys([]);
        setOriginalBinding("");
      }
    };

    const handleClickOutside = async (event: MouseEvent) => {
      if (cleanup) return;
      if (
        shortcutRef.current &&
        !shortcutRef.current.contains(event.target as Node)
      ) {
        if (editingShortcutId && originalBinding) {
          try {
            await updateBinding(editingShortcutId, originalBinding);
            await invoke("resume_binding", { id: editingShortcutId }).catch(
              console.error,
            );
          } catch (error) {
            console.error("Failed to restore original binding:", error);
          }
        } else if (editingShortcutId) {
          await invoke("resume_binding", { id: editingShortcutId }).catch(
            console.error,
          );
        }
        setEditingShortcutId(null);
        setKeyPressed([]);
        setRecordedKeys([]);
        setOriginalBinding("");
      }
    };

    window.addEventListener("keydown", handleKeyDown);
    window.addEventListener("keyup", handleKeyUp);
    window.addEventListener("click", handleClickOutside);

    return () => {
      cleanup = true;
      window.removeEventListener("keydown", handleKeyDown);
      window.removeEventListener("keyup", handleKeyUp);
      window.removeEventListener("click", handleClickOutside);
    };
  }, [
    keyPressed,
    recordedKeys,
    editingShortcutId,
    bindings,
    originalBinding,
    updateBinding,
    osType,
    bindingId,
  ]);

  const startRecording = async () => {
    if (editingShortcutId === bindingId) return;

    await invoke("suspend_binding", { id: bindingId }).catch(console.error);

    setOriginalBinding(binding?.current_binding || "");
    setEditingShortcutId(bindingId);
    setKeyPressed([]);
    setRecordedKeys([]);
  };

  const formatCurrentKeys = (): string => {
    if (recordedKeys.length === 0) return "Press keys...";
    return formatKeyCombination(recordedKeys.join("+"), osType);
  };

  const handleReset = async () => {
    await resetBinding(bindingId);
  };

  if (!enabled) {
    return (
      <div className="p-4 bg-mid-gray/5 rounded-lg border border-mid-gray/20">
        <p className="text-sm text-mid-gray text-center">
          Post-processing is currently disabled. Enable it in Debug settings to
          configure a dedicated hotkey.
        </p>
      </div>
    );
  }

  if (isLoading) {
    return (
      <SettingContainer
        title="Post-Processing Hotkey"
        description="Optional: Set a dedicated keyboard shortcut that always applies post-processing"
        descriptionMode={descriptionMode}
        grouped={grouped}
      >
        <div className="text-sm text-mid-gray">Loading...</div>
      </SettingContainer>
    );
  }

  const hasBinding = binding.current_binding && binding.current_binding !== "";
  const isEditing = editingShortcutId === bindingId;

  return (
    <SettingContainer
      title="Post-Processing Hotkey"
      description="Optional: Set a dedicated keyboard shortcut that always applies post-processing, regardless of the global setting"
      descriptionMode={descriptionMode}
      grouped={grouped}
      tooltipPosition="bottom"
    >
      <div className="flex items-center space-x-1">
        {isEditing ? (
          <div
            ref={shortcutRef}
            className="px-2 py-1 text-sm font-semibold border border-logo-primary bg-logo-primary/30 rounded min-w-[120px] text-center"
          >
            {formatCurrentKeys()}
          </div>
        ) : (
          <div
            className={`px-2 py-1 text-sm font-semibold rounded cursor-pointer min-w-[120px] text-center ${
              hasBinding
                ? "bg-mid-gray/10 border border-mid-gray/80 hover:bg-logo-primary/10 hover:border-logo-primary"
                : "bg-mid-gray/5 border border-mid-gray/40 hover:bg-logo-primary/10 hover:border-logo-primary text-mid-gray/60"
            }`}
            onClick={startRecording}
          >
            {hasBinding
              ? formatKeyCombination(binding.current_binding, osType)
              : "Not set"}
          </div>
        )}
        {hasBinding && (
          <ResetButton
            onClick={handleReset}
            disabled={isUpdating("bindings")}
            ariaLabel="Clear hotkey"
          />
        )}
      </div>
      {!hasBinding && (
        <p className="text-xs text-mid-gray/70 mt-2">
          Click to set a dedicated hotkey for post-processing. Leave unset to
          use the standard transcribe hotkey with the global post-processing
          setting.
        </p>
      )}
    </SettingContainer>
  );
};

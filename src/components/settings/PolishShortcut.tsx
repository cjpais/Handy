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

interface PolishShortcutProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const PolishShortcut: React.FC<PolishShortcutProps> = ({
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
  const shortcutRefs = useRef<Map<string, HTMLDivElement | null>>(new Map());

  const bindings = getSetting("bindings") || {};
  const polishBinding = bindings["polish"];

  // Detect and store OS type
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
    // Only add event listeners when we're in editing mode
    if (editingShortcutId === null) return;

    let cleanup = false;

    // Keyboard event listeners
    const handleKeyDown = async (e: KeyboardEvent) => {
      if (cleanup) return;
      if (e.repeat) return; // ignore auto-repeat
      if (e.key === "Escape") {
        // Cancel recording and restore original binding
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
      e.stopPropagation();

      const keyName = getKeyName(e, osType);
      const normalizedKey = normalizeKey(keyName);

      if (!keyPressed.includes(keyName)) {
        setKeyPressed((prev) => [...prev, keyName]);
      }
    };

    const handleKeyUp = async (e: KeyboardEvent) => {
      if (cleanup) return;
      if (e.repeat) return;

      e.preventDefault();
      e.stopPropagation();

      const keyName = getKeyName(e, osType);
      const normalizedKey = normalizeKey(keyName);

      // Remove the key from pressed keys
      setKeyPressed((prev) => prev.filter((k) => k !== keyName));

      // If all keys are released and we have recorded keys, finalize the shortcut
      const remainingKeys = keyPressed.filter((k) => k !== keyName);
      if (remainingKeys.length === 0 && recordedKeys.length > 0) {
        const shortcutString = recordedKeys.join("+");

        try {
          if (editingShortcutId) {
            await updateBinding(editingShortcutId, shortcutString);
            await invoke("resume_binding", { id: editingShortcutId }).catch(
              console.error,
            );
            toast.success("Polish shortcut updated successfully");
          }
        } catch (error) {
          console.error("Failed to update binding:", error);
          toast.error("Failed to update polish shortcut");
        }

        setEditingShortcutId(null);
        setKeyPressed([]);
        setRecordedKeys([]);
        setOriginalBinding("");
      }
    };

    const handleClickOutside = async (e: MouseEvent) => {
      if (cleanup) return;

      const target = e.target as Element;
      const isClickingShortcut = Array.from(shortcutRefs.current.values()).some(
        (ref) => ref && ref.contains(target),
      );

      if (!isClickingShortcut && editingShortcutId) {
        // Cancel recording and restore original binding
        if (originalBinding) {
          try {
            await updateBinding(editingShortcutId, originalBinding);
            await invoke("resume_binding", { id: editingShortcutId }).catch(
              console.error,
            );
          } catch (error) {
            console.error("Failed to restore original binding:", error);
            toast.error("Failed to restore original shortcut");
          }
        } else {
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

    // Update recorded keys whenever keyPressed changes
    if (keyPressed.length > 0) {
      setRecordedKeys([...keyPressed]);
    }

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
  ]);

  // Start recording a new shortcut
  const startRecording = async (id: string) => {
    if (editingShortcutId === id) return; // Already editing this shortcut

    // Suspend current binding to avoid firing while recording
    await invoke("suspend_binding", { id }).catch(console.error);

    // Store the original binding to restore if canceled
    setOriginalBinding(bindings[id]?.current_binding || "");
    setEditingShortcutId(id);
    setKeyPressed([]);
    setRecordedKeys([]);
  };

  // Format the current shortcut keys being recorded
  const formatCurrentKeys = (): string => {
    if (recordedKeys.length === 0) return "Press keys...";

    // Use the same formatting as the display to ensure consistency
    return formatKeyCombination(recordedKeys.join("+"), osType);
  };

  // Store references to shortcut elements
  const setShortcutRef = (id: string, ref: HTMLDivElement | null) => {
    shortcutRefs.current.set(id, ref);
  };

  // If still loading, show loading state
  if (isLoading) {
    return (
      <SettingContainer
        title="Polish Shortcut"
        description="Configure keyboard shortcut to apply polish rules to selected text"
        descriptionMode={descriptionMode}
        grouped={grouped}
      >
        <div className="text-sm text-mid-gray">Loading shortcuts...</div>
      </SettingContainer>
    );
  }

  // If no polish binding exists, show empty state
  if (!polishBinding) {
    return (
      <SettingContainer
        title="Polish Shortcut"
        description="Configure keyboard shortcut to apply polish rules to selected text"
        descriptionMode={descriptionMode}
        grouped={grouped}
      >
        <div className="text-sm text-mid-gray">Polish shortcut not configured</div>
      </SettingContainer>
    );
  }

  return (
    <SettingContainer
      title="Polish Shortcut"
      description="Set the keyboard shortcut to apply polish rules to selected text"
      descriptionMode={descriptionMode}
      grouped={grouped}
    >
      <div className="flex items-center space-x-1">
        {editingShortcutId === "polish" ? (
          <div
            ref={(ref) => setShortcutRef("polish", ref)}
            className="px-2 py-1 text-sm font-semibold border border-logo-primary bg-logo-primary/30 rounded min-w-[120px] text-center"
          >
            {formatCurrentKeys()}
          </div>
        ) : (
          <div
            className="px-2 py-1 text-sm font-semibold bg-mid-gray/10 border border-mid-gray/80 hover:bg-logo-primary/10 rounded cursor-pointer hover:border-logo-primary"
            onClick={() => startRecording("polish")}
          >
            {formatKeyCombination(polishBinding.current_binding, osType)}
          </div>
        )}
        <ResetButton
          onClick={() => resetBinding("polish")}
          disabled={isUpdating("binding_polish")}
        />
      </div>
    </SettingContainer>
  );
};
import React, { useEffect, useState, useRef, useMemo } from "react";
import { type } from "@tauri-apps/plugin-os";
import {
  getKeyName,
  formatKeyCombination,
  normalizeKey,
  type OSType,
} from "../../lib/utils/keyboard";
import { ResetButton } from "../ui/ResetButton";
import { SettingContainer } from "../ui/SettingContainer";
import { LanguageDropdown } from "../ui/LanguageDropdown";
import { useSettings } from "../../hooks/useSettings";
import { invoke } from "@tauri-apps/api/core";
import { toast } from "sonner";

interface HandyShortcutProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
  disableLanguageSelection?: boolean;
}

export const HandyShortcut: React.FC<HandyShortcutProps> = ({
  descriptionMode = "tooltip",
  grouped = false,
  disableLanguageSelection = false,
}) => {
  const { getSetting, updateBinding, resetBinding, isUpdating, isLoading, refreshSettings } =
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

  const sortedBindings = useMemo(() => {
    return Object.entries(bindings).sort(([idA], [idB]) => {
      // Put "transcribe" first, then sort others alphabetically
      if (idA === "transcribe") return -1;
      if (idB === "transcribe") return 1;
      return idA.localeCompare(idB);
    });
  }, [bindings]);

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

      // Get the key with OS-specific naming and normalize it
      const rawKey = getKeyName(e, osType);
      const key = normalizeKey(rawKey);

      if (!keyPressed.includes(key)) {
        setKeyPressed((prev) => [...prev, key]);
        // Also add to recorded keys if not already there
        if (!recordedKeys.includes(key)) {
          setRecordedKeys((prev) => [...prev, key]);
        }
      }
    };

    const handleKeyUp = async (e: KeyboardEvent) => {
      if (cleanup) return;
      e.preventDefault();

      // Get the key with OS-specific naming and normalize it
      const rawKey = getKeyName(e, osType);
      const key = normalizeKey(rawKey);

      // Remove from currently pressed keys
      setKeyPressed((prev) => prev.filter((k) => k !== key));

      // If no keys are pressed anymore, commit the shortcut
      const updatedKeyPressed = keyPressed.filter((k) => k !== key);
      if (updatedKeyPressed.length === 0 && recordedKeys.length > 0) {
        // Create the shortcut string from all recorded keys
        const newShortcut = recordedKeys.join("+");

        if (editingShortcutId && bindings[editingShortcutId]) {
          try {
            await updateBinding(editingShortcutId, newShortcut);
            // Re-register the shortcut now that recording is finished
            await invoke("resume_binding", { id: editingShortcutId }).catch(
              console.error,
            );
          } catch (error) {
            console.error("Failed to change binding:", error);
            toast.error(`Failed to set shortcut: ${error}`);

            // Reset to original binding on error
            if (originalBinding) {
              try {
                await updateBinding(editingShortcutId, originalBinding);
                await invoke("resume_binding", { id: editingShortcutId }).catch(
                  console.error,
                );
              } catch (resetError) {
                console.error("Failed to reset binding:", resetError);
                toast.error("Failed to reset shortcut to original value");
              }
            }
          }

          // Exit editing mode and reset states
          setEditingShortcutId(null);
          setKeyPressed([]);
          setRecordedKeys([]);
          setOriginalBinding("");
        }
      }
    };

    // Add click outside handler
    const handleClickOutside = async (e: MouseEvent) => {
      if (cleanup) return;
      const activeElement = shortcutRefs.current.get(editingShortcutId);
      if (activeElement && !activeElement.contains(e.target as Node)) {
        // Cancel shortcut recording and restore original binding
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
          invoke("resume_binding", { id: editingShortcutId }).catch(
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

  // Update language for a binding
  const updateBindingLanguage = async (id: string, language: string | null) => {
    try {
      // Convert null to "auto" for the backend
      const languageValue = language || "auto";
      await invoke("change_binding_language", { id, language: languageValue });
      toast.success("Language updated successfully");
      // Refresh settings to show the updated language
      await refreshSettings();
    } catch (error) {
      console.error("Failed to update binding language:", error);
      toast.error("Failed to update language");
    }
  };

  // Add a new shortcut
  const handleAddShortcut = async () => {
    try {
      await invoke("add_shortcut_binding");
      toast.success("Shortcut added successfully");

      // Refresh settings to show the new shortcut
      await refreshSettings();
    } catch (error) {
      console.error("Failed to add shortcut:", error);
      toast.error(`Failed to add shortcut: ${error}`);
    }
  };

  // Remove a shortcut
  const removeShortcut = async (id: string, name: string) => {
    try {
      await invoke("remove_shortcut_binding", { id });
      toast.success(`"${name}" removed successfully`);
      // Refresh settings to remove the shortcut from UI
      await refreshSettings();
    } catch (error) {
      console.error("Failed to remove shortcut:", error);
      toast.error(`Failed to remove shortcut: ${error}`);
    }
  };

  // If still loading, show loading state
  if (isLoading) {
    return (
      <SettingContainer
        title="Handy Shortcuts"
        description="Configure keyboard shortcuts to trigger speech-to-text recording"
        descriptionMode={descriptionMode}
        grouped={grouped}
      >
        <div className="text-sm text-mid-gray">Loading shortcuts...</div>
      </SettingContainer>
    );
  }

  // If no bindings are loaded, show empty state
  if (Object.keys(bindings).length === 0) {
    return (
      <SettingContainer
        title="Handy Shortcuts"
        description="Configure keyboard shortcuts to trigger speech-to-text recording"
        descriptionMode={descriptionMode}
        grouped={grouped}
      >
        <div className="text-sm text-mid-gray">No shortcuts configured</div>
      </SettingContainer>
    );
  }

  return (
    <SettingContainer
      title="Handy Shortcuts"
      description="Configure keyboard shortcuts and languages for speech-to-text recording"
      descriptionMode={descriptionMode}
      grouped={grouped}
      layout="stacked"
    >
      <div className="flex flex-col space-y-4">
        {sortedBindings.map(([bindingId, binding]) => {
          const selectedLanguage = binding.language || null;

          return (
            <div key={bindingId} className="flex flex-col space-y-2 p-3 border border-mid-gray/30 rounded-lg">
              <div className="flex items-center justify-between">
                <div className="text-sm font-medium text-foreground">
                  {binding.name}
                </div>
                {bindingId !== "transcribe" && (
                  <button
                    onClick={(e) => {
                      e.stopPropagation();
                      removeShortcut(bindingId, binding.name);
                    }}
                    className="px-2 py-1 text-xs text-red-600 hover:text-red-700 hover:bg-red-50 rounded"
                    title="Remove shortcut"
                  >
                    Remove
                  </button>
                )}
              </div>
              <div className="flex items-center space-x-1">
                {editingShortcutId === bindingId ? (
                  <div
                    ref={(ref) => setShortcutRef(bindingId, ref)}
                    className="px-2 py-1 text-sm font-semibold border border-logo-primary bg-logo-primary/30 rounded min-w-[120px] text-center"
                  >
                    {formatCurrentKeys()}
                  </div>
                ) : (
                  <div
                    className="px-2 py-1 text-sm font-semibold bg-mid-gray/10 border border-mid-gray/80 hover:bg-logo-primary/10 rounded cursor-pointer hover:border-logo-primary"
                    onClick={() => startRecording(bindingId)}
                  >
                    {formatKeyCombination(binding.current_binding, osType)}
                  </div>
                )}
                <ResetButton
                  onClick={() => resetBinding(bindingId)}
                  disabled={isUpdating(`binding_${bindingId}`)}
                />
              </div>
              <div className="flex flex-col space-y-1">
                <span className="text-sm font-medium text-foreground">Language</span>
                <LanguageDropdown
                  value={selectedLanguage}
                  onChange={(language) => updateBindingLanguage(bindingId, language)}
                  disabled={disableLanguageSelection}
                />
              </div>
            </div>
          );
        })}

        <button
          onClick={handleAddShortcut}
          className="px-4 py-2 text-sm font-medium text-logo-primary border border-logo-primary hover:bg-logo-primary/10 rounded-lg transition-colors"
        >
          + Add Shortcut
        </button>
      </div>
    </SettingContainer>
  );
};

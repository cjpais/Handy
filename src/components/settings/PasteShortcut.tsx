import React, { useCallback, useEffect, useRef, useState } from "react";
import { toast } from "sonner";
import { useSettings } from "../../hooks/useSettings";
import { SettingContainer } from "../ui/SettingContainer";
import { ResetButton } from "../ui/ResetButton";
import {
  formatKeyCombination,
  getKeyName,
  normalizeKey,
  type OSType,
} from "../../lib/utils/keyboard";
import { type } from "@tauri-apps/plugin-os";

export interface PasteShortcutProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const PasteShortcut: React.FC<PasteShortcutProps> = ({
  descriptionMode = "tooltip",
  grouped = false,
}) => {
  const {
    settings,
    isLoading,
    isUpdating,
    updatePasteBinding,
    resetPasteBinding,
  } = useSettings();

  const currentBinding = settings?.paste_binding ?? "";
  const [editing, setEditing] = useState(false);
  const [recordedKeys, setRecordedKeys] = useState<string[]>([]);
  const [osType, setOsType] = useState<OSType>("unknown");
  const containerRef = useRef<HTMLDivElement | null>(null);
  const recordedKeysRef = useRef<string[]>([]);
  const pressedKeysRef = useRef<Set<string>>(new Set());

  // Detect OS once
  useEffect(() => {
    let isMounted = true;

    const detectOsType = async () => {
      try {
        const detected = await type();
        if (!isMounted) return;
        switch (detected) {
          case "macos":
          case "windows":
          case "linux":
            setOsType(detected);
            break;
          default:
            setOsType("unknown");
        }
      } catch (error) {
        console.error("Failed to detect OS type", error);
        if (isMounted) {
          setOsType("unknown");
        }
      }
    };

    detectOsType();

    return () => {
      isMounted = false;
    };
  }, []);

  const resetRecordingState = useCallback(() => {
    pressedKeysRef.current.clear();
    recordedKeysRef.current = [];
    setRecordedKeys([]);
  }, []);

  const cancelEditing = useCallback(() => {
    setEditing(false);
    resetRecordingState();
  }, [resetRecordingState]);

  const commitBinding = useCallback(async () => {
    if (recordedKeysRef.current.length === 0) {
      cancelEditing();
      return;
    }

    const combination = recordedKeysRef.current.join("+");

    try {
      await updatePasteBinding(combination);
    } catch (error) {
      console.error("Failed to update paste binding", error);
      toast.error("Failed to update paste shortcut");
    } finally {
      cancelEditing();
    }
  }, [cancelEditing, updatePasteBinding]);

  useEffect(() => {
    if (!editing) return;

    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.repeat) return;
      if (event.key === "Escape") {
        event.preventDefault();
        cancelEditing();
        return;
      }

      event.preventDefault();
      const raw = getKeyName(event, osType);
      const normalized = normalizeKey(raw);

      if (!pressedKeysRef.current.has(normalized)) {
        pressedKeysRef.current.add(normalized);
      }

      if (!recordedKeysRef.current.includes(normalized)) {
        recordedKeysRef.current = [...recordedKeysRef.current, normalized];
        setRecordedKeys([...recordedKeysRef.current]);
      }
    };

    const handleKeyUp = (event: KeyboardEvent) => {
      event.preventDefault();
      const raw = getKeyName(event, osType);
      const normalized = normalizeKey(raw);

      if (pressedKeysRef.current.has(normalized)) {
        pressedKeysRef.current.delete(normalized);
      }

      if (pressedKeysRef.current.size === 0) {
        void commitBinding();
      }
    };

    const handleClickOutside = (event: MouseEvent) => {
      if (containerRef.current && !containerRef.current.contains(event.target as Node)) {
        cancelEditing();
      }
    };

    window.addEventListener("keydown", handleKeyDown);
    window.addEventListener("keyup", handleKeyUp);
    window.addEventListener("mousedown", handleClickOutside);

    return () => {
      window.removeEventListener("keydown", handleKeyDown);
      window.removeEventListener("keyup", handleKeyUp);
      window.removeEventListener("mousedown", handleClickOutside);
    };
  }, [editing, osType, commitBinding, cancelEditing]);

  const startEditing = () => {
    if (editing) return;
    resetRecordingState();
    setEditing(true);
  };

  const handleReset = async () => {
    try {
      cancelEditing();
      await resetPasteBinding();
    } catch (error) {
      console.error("Failed to reset paste binding", error);
      toast.error("Failed to reset paste shortcut");
    }
  };

  const displayValue = editing
    ? recordedKeys.length > 0
      ? formatKeyCombination(recordedKeys.join("+"), osType)
      : "Press keys..."
    : currentBinding
    ? formatKeyCombination(currentBinding, osType)
    : "Not set";

  const isBusy = isUpdating("paste_binding");

  if (isLoading) {
    return (
      <SettingContainer
        title="Paste Shortcut"
        description="Configure the key combination used to paste transcribed text"
        descriptionMode={descriptionMode}
        grouped={grouped}
      >
        <div className="text-sm text-mid-gray">Loading…</div>
      </SettingContainer>
    );
  }

  return (
    <SettingContainer
      title="Paste Shortcut"
      description="Choose the key combination Handy should send when pasting the transcription"
      descriptionMode={descriptionMode}
      grouped={grouped}
    >
      <div
        ref={containerRef}
        className="flex items-center gap-2"
      >
        <button
          type="button"
          className={`px-2 py-1 text-sm font-semibold rounded border transition-colors duration-150 ${
            editing
              ? "border-logo-primary bg-logo-primary/30"
              : "border-mid-gray/80 bg-mid-gray/10 hover:border-logo-primary hover:bg-logo-primary/10"
          }`}
          onClick={startEditing}
          disabled={isBusy}
        >
          {displayValue}
        </button>
        <ResetButton onClick={handleReset} disabled={isBusy} />
      </div>
    </SettingContainer>
  );
};

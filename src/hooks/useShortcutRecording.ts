import { useEffect, useState, useRef, useCallback } from "react";
import { listen } from "@tauri-apps/api/event";
import { type } from "@tauri-apps/plugin-os";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { commands } from "@/bindings";
import { formatKeyCombination, type OSType } from "../lib/utils/keyboard";

// Event types from the Rust backend
interface KeyboardRecordEvent {
  binding_id: string;
  modifiers: string[];
  key: string | null;
  current_combo: string;
}

interface KeyboardRecordingComplete {
  binding_id: string;
  shortcut: string;
}

interface KeyboardRecordingCancelled {
  binding_id: string;
}

interface UseShortcutRecordingOptions {
  shortcutId: string;
  currentBinding: string;
  onBindingChange: (newBinding: string) => Promise<void>;
}

interface UseShortcutRecordingReturn {
  isRecording: boolean;
  currentCombo: string;
  osType: OSType;
  shortcutRef: React.RefObject<HTMLDivElement>;
  startRecording: () => Promise<void>;
  formatCurrentKeys: () => string;
}

export const useShortcutRecording = ({
  shortcutId,
  currentBinding,
  onBindingChange,
}: UseShortcutRecordingOptions): UseShortcutRecordingReturn => {
  const { t } = useTranslation();
  const [currentCombo, setCurrentCombo] = useState<string>("");
  const [isRecording, setIsRecording] = useState(false);
  const [originalBinding, setOriginalBinding] = useState<string>("");
  const [osType, setOsType] = useState<OSType>("unknown");
  const shortcutRef = useRef<HTMLDivElement>(null);

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

  // Handle recording completion
  const handleRecordingComplete = useCallback(
    async (newShortcut: string) => {
      try {
        await onBindingChange(newShortcut);
        // Re-register the shortcut now that recording is finished
        await commands.resumeBinding(shortcutId).catch(console.error);
      } catch (error) {
        console.error("Failed to change binding:", error);
        toast.error(
          t("settings.general.shortcut.errors.set", {
            error: String(error),
          })
        );

        // Reset to original binding on error
        if (originalBinding) {
          try {
            await onBindingChange(originalBinding);
            await commands.resumeBinding(shortcutId).catch(console.error);
          } catch (resetError) {
            console.error("Failed to reset binding:", resetError);
            toast.error(t("settings.general.shortcut.errors.reset"));
          }
        }
      }

      setIsRecording(false);
      setCurrentCombo("");
      setOriginalBinding("");
    },
    [shortcutId, originalBinding, onBindingChange, t]
  );

  // Cancel recording and restore original binding
  const cancelRecording = useCallback(async () => {
    try {
      // Stop the backend recording
      await commands.cancelKeyboardRecording(shortcutId).catch(console.error);

      // Restore original binding if we have one
      if (originalBinding) {
        await onBindingChange(originalBinding);
      }

      // Resume the original shortcut
      await commands.resumeBinding(shortcutId).catch(console.error);
    } catch (error) {
      console.error("Failed to cancel recording:", error);
      toast.error(t("settings.general.shortcut.errors.restore"));
    }

    setIsRecording(false);
    setCurrentCombo("");
    setOriginalBinding("");
  }, [shortcutId, originalBinding, onBindingChange, t]);

  // Listen for keyboard recording events from the backend
  useEffect(() => {
    if (!isRecording) return;

    let unlistenKeyDown: (() => void) | undefined;
    let unlistenComplete: (() => void) | undefined;
    let unlistenCancelled: (() => void) | undefined;

    const setupListeners = async () => {
      // Listen for key-down events to show current combo
      unlistenKeyDown = await listen<KeyboardRecordEvent>(
        "keyboard:key-down",
        (event) => {
          if (event.payload.binding_id === shortcutId) {
            setCurrentCombo(event.payload.current_combo);
          }
        }
      );

      // Listen for recording complete
      unlistenComplete = await listen<KeyboardRecordingComplete>(
        "keyboard:recording-complete",
        async (event) => {
          if (event.payload.binding_id === shortcutId) {
            await handleRecordingComplete(event.payload.shortcut);
          }
        }
      );

      // Listen for recording cancelled
      unlistenCancelled = await listen<KeyboardRecordingCancelled>(
        "keyboard:recording-cancelled",
        (event) => {
          if (event.payload.binding_id === shortcutId) {
            setIsRecording(false);
            setCurrentCombo("");
            setOriginalBinding("");
          }
        }
      );
    };

    setupListeners();

    return () => {
      unlistenKeyDown?.();
      unlistenComplete?.();
      unlistenCancelled?.();
    };
  }, [isRecording, shortcutId, handleRecordingComplete]);

  // Handle click outside to cancel recording
  useEffect(() => {
    if (!isRecording) return;

    const handleClickOutside = async (e: MouseEvent) => {
      if (
        shortcutRef.current &&
        !shortcutRef.current.contains(e.target as Node)
      ) {
        await cancelRecording();
      }
    };

    // Handle Escape key to cancel recording
    const handleKeyDown = async (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        await cancelRecording();
      }
    };

    window.addEventListener("click", handleClickOutside);
    window.addEventListener("keydown", handleKeyDown);

    return () => {
      window.removeEventListener("click", handleClickOutside);
      window.removeEventListener("keydown", handleKeyDown);
    };
  }, [isRecording, cancelRecording]);

  // Start recording a new shortcut
  const startRecording = useCallback(async () => {
    if (isRecording) return;

    try {
      // Suspend current binding to avoid firing while recording
      await commands.suspendBinding(shortcutId).catch(console.error);

      // Store the original binding to restore if canceled
      setOriginalBinding(currentBinding);
      setIsRecording(true);
      setCurrentCombo("");

      // Start the backend keyboard recording
      await commands.startKeyboardRecording(shortcutId);
    } catch (error) {
      console.error("Failed to start recording:", error);
      toast.error(t("settings.general.shortcut.errors.startRecording"));
      setIsRecording(false);
    }
  }, [isRecording, shortcutId, currentBinding, t]);

  // Format the current shortcut keys being recorded
  const formatCurrentKeys = useCallback((): string => {
    if (!currentCombo) {
      return t("settings.general.shortcut.pressKeys");
    }
    return formatKeyCombination(currentCombo, osType);
  }, [currentCombo, osType, t]);

  return {
    isRecording,
    currentCombo,
    osType,
    shortcutRef,
    startRecording,
    formatCurrentKeys,
  };
};

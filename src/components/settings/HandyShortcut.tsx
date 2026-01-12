import React, { useEffect, useState, useRef } from "react";
import { useTranslation } from "react-i18next";
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
import { commands } from "@/bindings";
import { toast } from "sonner";
import { AlertCircle } from "lucide-react";

interface HandyShortcutProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
  shortcutId: string;
  disabled?: boolean;
}

export const HandyShortcut: React.FC<HandyShortcutProps> = ({
  descriptionMode = "tooltip",
  grouped = false,
  shortcutId,
  disabled = false,
}) => {
  const { t } = useTranslation();
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

  // Wayland-specific state
  const [isWayland, setIsWayland] = useState(false);
  const [gnomeShortcut, setGnomeShortcut] = useState<string | null>(null);
  const [isConfiguringGnome, setIsConfiguringGnome] = useState(false);
  const [gnomeRecordedKeys, setGnomeRecordedKeys] = useState<string[]>([]);

  // Detect Wayland session
  useEffect(() => {
    const checkWayland = async () => {
      try {
        const wayland = await commands.isWaylandSession();
        setIsWayland(wayland);
        if (wayland) {
          // Get current GNOME shortcut
          const result = await commands.getGnomeShortcut();
          if (result.status === "ok" && result.data) {
            setGnomeShortcut(result.data);
          }
        }
      } catch (error) {
        console.error("Error checking Wayland session:", error);
      }
    };
    checkWayland();
  }, []);

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
            await commands
              .resumeBinding(editingShortcutId)
              .catch(console.error);
          } catch (error) {
            console.error("Failed to restore original binding:", error);
            toast.error(t("settings.general.shortcut.errors.restore"));
          }
        } else if (editingShortcutId) {
          await commands.resumeBinding(editingShortcutId).catch(console.error);
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
            await commands
              .resumeBinding(editingShortcutId)
              .catch(console.error);
          } catch (error) {
            console.error("Failed to change binding:", error);
            toast.error(
              t("settings.general.shortcut.errors.set", {
                error: String(error),
              }),
            );

            // Reset to original binding on error
            if (originalBinding) {
              try {
                await updateBinding(editingShortcutId, originalBinding);
                await commands
                  .resumeBinding(editingShortcutId)
                  .catch(console.error);
              } catch (resetError) {
                console.error("Failed to reset binding:", resetError);
                toast.error(t("settings.general.shortcut.errors.reset"));
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
            await commands
              .resumeBinding(editingShortcutId)
              .catch(console.error);
          } catch (error) {
            console.error("Failed to restore original binding:", error);
            toast.error(t("settings.general.shortcut.errors.restore"));
          }
        } else if (editingShortcutId) {
          commands.resumeBinding(editingShortcutId).catch(console.error);
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
    await commands.suspendBinding(id).catch(console.error);

    // Store the original binding to restore if canceled
    setOriginalBinding(bindings[id]?.current_binding || "");
    setEditingShortcutId(id);
    setKeyPressed([]);
    setRecordedKeys([]);
  };

  // Format the current shortcut keys being recorded
  const formatCurrentKeys = (): string => {
    if (recordedKeys.length === 0)
      return t("settings.general.shortcut.pressKeys");

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
        title={t("settings.general.shortcut.title")}
        description={t("settings.general.shortcut.description")}
        descriptionMode={descriptionMode}
        grouped={grouped}
      >
        <div className="text-sm text-mid-gray">
          {t("settings.general.shortcut.loading")}
        </div>
      </SettingContainer>
    );
  }

  // If no bindings are loaded, show empty state
  if (Object.keys(bindings).length === 0) {
    return (
      <SettingContainer
        title={t("settings.general.shortcut.title")}
        description={t("settings.general.shortcut.description")}
        descriptionMode={descriptionMode}
        grouped={grouped}
      >
        <div className="text-sm text-mid-gray">
          {t("settings.general.shortcut.none")}
        </div>
      </SettingContainer>
    );
  }

  const binding = bindings[shortcutId];
  if (!binding) {
    return (
      <SettingContainer
        title={t("settings.general.shortcut.title")}
        description={t("settings.general.shortcut.notFound")}
        descriptionMode={descriptionMode}
        grouped={grouped}
      >
        <div className="text-sm text-mid-gray">
          {t("settings.general.shortcut.none")}
        </div>
      </SettingContainer>
    );
  }

  // Get translated name and description for the binding
  const translatedName = t(
    `settings.general.shortcut.bindings.${shortcutId}.name`,
    binding.name,
  );
  const translatedDescription = t(
    `settings.general.shortcut.bindings.${shortcutId}.description`,
    binding.description,
  );

  // Handle GNOME shortcut recording for Wayland
  const startGnomeRecording = () => {
    setIsConfiguringGnome(true);
    setGnomeRecordedKeys([]);
  };

  const handleGnomeKeyDown = async (e: React.KeyboardEvent) => {
    if (!isConfiguringGnome) return;
    e.preventDefault();

    if (e.key === "Escape") {
      setIsConfiguringGnome(false);
      setGnomeRecordedKeys([]);
      return;
    }

    const rawKey = getKeyName(e.nativeEvent, osType);
    const key = normalizeKey(rawKey);

    if (!gnomeRecordedKeys.includes(key)) {
      setGnomeRecordedKeys((prev) => [...prev, key]);
    }
  };

  const handleGnomeKeyUp = async (e: React.KeyboardEvent) => {
    if (!isConfiguringGnome) return;
    e.preventDefault();

    const rawKey = getKeyName(e.nativeEvent, osType);
    const key = normalizeKey(rawKey);

    // Remove from currently pressed keys check
    const remainingKeys = gnomeRecordedKeys.filter((k) => k !== key);

    // If all keys released, save the shortcut
    if (gnomeRecordedKeys.length > 0) {
      // Convert to GNOME format: <Control><Shift>space
      const gnomeFormat = gnomeRecordedKeys
        .map((k) => {
          const lower = k.toLowerCase();
          if (lower === "ctrl" || lower === "control") return "<Control>";
          if (lower === "shift") return "<Shift>";
          if (lower === "alt") return "<Alt>";
          if (lower === "super" || lower === "meta") return "<Super>";
          return k.toLowerCase();
        })
        .join("");

      try {
        const result = await commands.configureGnomeShortcut(gnomeFormat);
        if (result.status === "ok") {
          setGnomeShortcut(gnomeFormat);
          toast.success(t("settings.general.shortcut.wayland.configured"));
        } else {
          toast.error(t("settings.general.shortcut.wayland.error"));
        }
      } catch (error) {
        console.error("Failed to configure GNOME shortcut:", error);
        toast.error(t("settings.general.shortcut.wayland.error"));
      }

      setIsConfiguringGnome(false);
      setGnomeRecordedKeys([]);
    }
  };

  // Format GNOME shortcut for display
  const formatGnomeShortcut = (shortcut: string | null): string => {
    if (!shortcut) return t("settings.general.shortcut.wayland.notConfigured");
    return shortcut
      .replace(/<Control>/g, "Ctrl+")
      .replace(/<Shift>/g, "Shift+")
      .replace(/<Alt>/g, "Alt+")
      .replace(/<Super>/g, "Super+")
      .replace(/\+$/, "");
  };

  // Wayland-specific UI
  if (isWayland && shortcutId === "transcribe") {
    return (
      <SettingContainer
        title={translatedName}
        description={t("settings.general.shortcut.wayland.description")}
        descriptionMode={descriptionMode}
        grouped={grouped}
        disabled={disabled}
        layout="stacked"
      >
        <div className="space-y-3">
          <div className="flex items-start gap-2 p-3 bg-amber-500/10 border border-amber-500/30 rounded-lg">
            <AlertCircle className="w-5 h-5 text-amber-500 shrink-0 mt-0.5" />
            <p className="text-sm text-amber-200">
              {t("settings.general.shortcut.wayland.notice")}
            </p>
          </div>

          <div className="flex items-center space-x-2">
            {isConfiguringGnome ? (
              <div
                tabIndex={0}
                onKeyDown={handleGnomeKeyDown}
                onKeyUp={handleGnomeKeyUp}
                className="px-3 py-2 text-sm font-semibold border border-logo-primary bg-logo-primary/30 rounded min-w-[180px] text-center focus:outline-none"
                autoFocus
              >
                {gnomeRecordedKeys.length === 0
                  ? t("settings.general.shortcut.pressKeys")
                  : gnomeRecordedKeys.join("+")}
              </div>
            ) : (
              <div
                className="px-3 py-2 text-sm font-semibold bg-mid-gray/10 border border-mid-gray/80 hover:bg-logo-primary/10 rounded cursor-pointer hover:border-logo-primary min-w-[180px] text-center"
                onClick={startGnomeRecording}
              >
                {formatGnomeShortcut(gnomeShortcut)}
              </div>
            )}
          </div>

          <p className="text-xs text-mid-gray">
            {t("settings.general.shortcut.wayland.hint")}
          </p>
        </div>
      </SettingContainer>
    );
  }

  return (
    <SettingContainer
      title={translatedName}
      description={translatedDescription}
      descriptionMode={descriptionMode}
      grouped={grouped}
      disabled={disabled}
      layout="horizontal"
    >
      <div className="flex items-center space-x-1">
        {editingShortcutId === shortcutId ? (
          <div
            ref={(ref) => setShortcutRef(shortcutId, ref)}
            className="px-2 py-1 text-sm font-semibold border border-logo-primary bg-logo-primary/30 rounded min-w-[120px] text-center"
          >
            {formatCurrentKeys()}
          </div>
        ) : (
          <div
            className="px-2 py-1 text-sm font-semibold bg-mid-gray/10 border border-mid-gray/80 hover:bg-logo-primary/10 rounded cursor-pointer hover:border-logo-primary"
            onClick={() => startRecording(shortcutId)}
          >
            {formatKeyCombination(binding.current_binding, osType)}
          </div>
        )}
        <ResetButton
          onClick={() => resetBinding(shortcutId)}
          disabled={isUpdating(`binding_${shortcutId}`)}
        />
      </div>
    </SettingContainer>
  );
};

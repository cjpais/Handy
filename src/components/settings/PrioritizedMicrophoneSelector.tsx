import React, { useEffect } from "react";
import { useTranslation } from "react-i18next";
import { ChevronUp, ChevronDown, X, Plus, RefreshCw } from "lucide-react";
import { useSettings } from "../../hooks/useSettings";
import { ToggleSwitch } from "../ui/ToggleSwitch";
import Badge from "../ui/Badge";

export const PrioritizedMicrophoneSelector: React.FC = React.memo(() => {
  const { t } = useTranslation();
  const {
    getSetting,
    updateSetting,
    isUpdating,
    audioDevices,
    refreshAudioDevices,
  } = useSettings();

  const enabled = getSetting("multi_microphone_enabled") ?? false;
  const prioritized: string[] = getSetting("prioritized_microphones") ?? [];
  const availableNames = new Set(audioDevices.map((d) => d.name));

  const activeDevice =
    prioritized.find((name) => availableNames.has(name)) ?? null;

  const available = audioDevices
    .map((d) => d.name)
    .filter((name) => name !== "Default" && !prioritized.includes(name));

  const isDisabled =
    isUpdating("prioritized_microphones") ||
    isUpdating("multi_microphone_enabled");

  const handleToggle = (checked: boolean) => {
    updateSetting("multi_microphone_enabled", checked);
  };

  useEffect(() => {
    if (enabled) {
      refreshAudioDevices();
    }
    window.addEventListener("focus", refreshAudioDevices);
    return () => window.removeEventListener("focus", refreshAudioDevices);
  }, [refreshAudioDevices, enabled]);

  const handleAdd = (name: string) => {
    updateSetting("prioritized_microphones", [...prioritized, name]);
  };

  const handleRemove = (name: string) => {
    updateSetting(
      "prioritized_microphones",
      prioritized.filter((n) => n !== name),
    );
  };

  const handleMoveUp = (index: number) => {
    if (index === 0) return;
    const next = [...prioritized];
    [next[index - 1], next[index]] = [next[index], next[index - 1]];
    updateSetting("prioritized_microphones", next);
  };

  const handleMoveDown = (index: number) => {
    if (index === prioritized.length - 1) return;
    const next = [...prioritized];
    [next[index], next[index + 1]] = [next[index + 1], next[index]];
    updateSetting("prioritized_microphones", next);
  };

  return (
    <>
      <ToggleSwitch
        checked={enabled}
        onChange={handleToggle}
        label={t("settings.sound.multipleMicrophones.label")}
        description={t("settings.sound.multipleMicrophones.description")}
        descriptionMode="tooltip"
        grouped={true}
      />

      {enabled && (
        <>
          {/* Selected devices header */}
          <div className="flex items-center gap-2 px-4 p-2">
            <h4 className="text-xs font-medium text-mid-gray uppercase tracking-wide">
              {t("settings.sound.multipleMicrophones.selectedTitle")}
            </h4>
          </div>

          {/* Selected device rows */}
          {prioritized.length === 0 ? (
            <div className="px-4 p-2">
              <p className="text-xs text-mid-gray italic">
                {t("settings.sound.multipleMicrophones.empty")}
              </p>
            </div>
          ) : (
            prioritized.map((name, index) => {
              const isActive = name === activeDevice;
              return (
                <div
                  key={name}
                  className={`flex items-center gap-3 px-4 p-2 text-sm transition-colors ${
                    isActive ? "bg-logo-primary/5" : ""
                  }`}
                >
                  <span className="text-xs text-mid-gray w-4 text-center shrink-0 font-medium">
                    {index + 1}
                  </span>
                  <span className="flex-1 truncate">{name}</span>
                  {isActive && (
                    <Badge
                      variant="primary"
                      className="text-[10px] px-2 py-0.5"
                    >
                      {t("settings.sound.multipleMicrophones.active")}
                    </Badge>
                  )}
                  <div className="flex items-center shrink-0">
                    <button
                      onClick={() => handleMoveUp(index)}
                      disabled={isDisabled || index === 0}
                      aria-label={t(
                        "settings.sound.multipleMicrophones.moveUp",
                      )}
                      className="p-1 rounded hover:bg-mid-gray/10 text-mid-gray hover:text-text disabled:opacity-30 disabled:hover:bg-transparent disabled:hover:text-mid-gray transition-colors"
                    >
                      <ChevronUp size={14} />
                    </button>
                    <button
                      onClick={() => handleMoveDown(index)}
                      disabled={
                        isDisabled || index === prioritized.length - 1
                      }
                      aria-label={t(
                        "settings.sound.multipleMicrophones.moveDown",
                      )}
                      className="p-1 rounded hover:bg-mid-gray/10 text-mid-gray hover:text-text disabled:opacity-30 disabled:hover:bg-transparent disabled:hover:text-mid-gray transition-colors"
                    >
                      <ChevronDown size={14} />
                    </button>
                    <button
                      onClick={() => handleRemove(name)}
                      disabled={isDisabled}
                      aria-label={t(
                        "settings.sound.multipleMicrophones.remove",
                      )}
                      className="p-1 rounded hover:bg-red-500/10 text-mid-gray hover:text-red-500 disabled:opacity-30 disabled:hover:bg-transparent disabled:hover:text-mid-gray transition-colors ml-1"
                    >
                      <X size={14} />
                    </button>
                  </div>
                </div>
              );
            })
          )}

          {/* Available devices header */}
          <div className="flex items-center gap-2 px-4 p-2">
            <h4 className="text-xs font-medium text-mid-gray uppercase tracking-wide">
              {t("settings.sound.multipleMicrophones.availableTitle")}
            </h4>
            <button
              onClick={refreshAudioDevices}
              disabled={isDisabled}
              aria-label={t(
                "settings.sound.multipleMicrophones.refresh",
              )}
              className="p-0.5 rounded hover:bg-mid-gray/10 text-mid-gray hover:text-text disabled:opacity-30 transition-colors"
            >
              <RefreshCw size={12} />
            </button>
          </div>

          {/* Available device rows */}
          {available.length === 0 ? (
            <div className="px-4 p-2">
              <p className="text-xs text-mid-gray italic">
                {t("settings.sound.multipleMicrophones.noAvailable")}
              </p>
            </div>
          ) : (
            available.map((name) => (
              <div
                key={name}
                className="flex items-center gap-3 px-4 p-2 text-sm group"
              >
                <span className="flex-1 truncate text-mid-gray">
                  {name}
                </span>
                <button
                  onClick={() => handleAdd(name)}
                  disabled={isDisabled}
                  aria-label={t(
                    "settings.sound.multipleMicrophones.add",
                  )}
                  className="p-1 rounded hover:bg-logo-primary/10 text-mid-gray hover:text-logo-primary opacity-0 group-hover:opacity-100 disabled:opacity-30 transition-all shrink-0"
                >
                  <Plus size={14} />
                </button>
              </div>
            ))
          )}
        </>
      )}
    </>
  );
});

PrioritizedMicrophoneSelector.displayName = "PrioritizedMicrophoneSelector";

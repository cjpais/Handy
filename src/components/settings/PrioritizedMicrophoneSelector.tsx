import React, { useEffect } from "react";
import { useTranslation } from "react-i18next";
import { useSettings } from "../../hooks/useSettings";
import { Button } from "../ui/Button";
import Badge from "../ui/Badge";

interface PrioritizedMicrophoneSelectorProps {
  grouped?: boolean;
}

export const PrioritizedMicrophoneSelector: React.FC<PrioritizedMicrophoneSelectorProps> =
  React.memo(({ grouped = false }) => {
    const { t } = useTranslation();
    const {
      getSetting,
      updateSetting,
      isUpdating,
      audioDevices,
      refreshAudioDevices,
    } = useSettings();

    const prioritized: string[] = getSetting("prioritized_microphones") ?? [];
    const availableNames = new Set(audioDevices.map((d) => d.name));

    const activeDevice =
      prioritized.find((name) => availableNames.has(name)) ?? null;

    const available = audioDevices
      .map((d) => d.name)
      .filter((name) => name !== "Default" && !prioritized.includes(name));

    const isDisabled = isUpdating("prioritized_microphones");

    useEffect(() => {
      window.addEventListener("focus", refreshAudioDevices);
      return () => window.removeEventListener("focus", refreshAudioDevices);
    }, [refreshAudioDevices]);

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

    const containerClass = grouped
      ? "px-4 py-3 space-y-4"
      : "px-4 py-3 space-y-4 rounded-lg border border-mid-gray/20";

    return (
      <div className={containerClass}>
        <div>
          <h3 className="text-sm font-medium mb-1">
            {t("settings.sound.prioritizedMicrophones.title")}
          </h3>
          <p className="text-xs text-mid-gray mb-2">
            {t("settings.sound.prioritizedMicrophones.description")}
          </p>

          {prioritized.length === 0 ? (
            <p className="text-xs text-mid-gray italic py-1">
              {t("settings.sound.prioritizedMicrophones.empty")}
            </p>
          ) : (
            <ul className="space-y-1">
              {prioritized.map((name, index) => {
                const isActive = name === activeDevice;
                return (
                  <li
                    key={name}
                    className="flex items-center gap-2 py-1 text-sm"
                  >
                    <span className="text-mid-gray w-4 text-right shrink-0">
                      {index + 1}
                    </span>
                    <span className="flex-1 truncate">{name}</span>
                    {isActive && (
                      <Badge variant="success">
                        {t("settings.sound.prioritizedMicrophones.active")}
                      </Badge>
                    )}
                    <div className="flex items-center gap-1 shrink-0">
                      <Button
                        variant="ghost"
                        size="sm"
                        onClick={() => handleMoveUp(index)}
                        disabled={isDisabled || index === 0}
                        aria-label={t(
                          "settings.sound.prioritizedMicrophones.moveUp",
                        )}
                        className="px-1.5 py-0.5"
                      >
                        {"↑"}
                      </Button>
                      <Button
                        variant="ghost"
                        size="sm"
                        onClick={() => handleMoveDown(index)}
                        disabled={
                          isDisabled || index === prioritized.length - 1
                        }
                        aria-label={t(
                          "settings.sound.prioritizedMicrophones.moveDown",
                        )}
                        className="px-1.5 py-0.5"
                      >
                        {"↓"}
                      </Button>
                      <Button
                        variant="danger-ghost"
                        size="sm"
                        onClick={() => handleRemove(name)}
                        disabled={isDisabled}
                        aria-label={t(
                          "settings.sound.prioritizedMicrophones.remove",
                        )}
                        className="px-1.5 py-0.5"
                      >
                        {"—"}
                      </Button>
                    </div>
                  </li>
                );
              })}
            </ul>
          )}
        </div>

        <div>
          <div className="flex items-center gap-2 mb-2">
            <h3 className="text-sm font-medium">
              {t("settings.sound.prioritizedMicrophones.availableTitle")}
            </h3>
            <Button
              variant="ghost"
              size="sm"
              onClick={refreshAudioDevices}
              disabled={isDisabled}
              aria-label={t("settings.sound.prioritizedMicrophones.refresh")}
              className="px-1.5 py-0.5 text-mid-gray"
            >
              {"↻"}
            </Button>
          </div>

          {available.length === 0 ? (
            <p className="text-xs text-mid-gray italic py-1">
              {t("settings.sound.prioritizedMicrophones.noAvailable")}
            </p>
          ) : (
            <ul className="space-y-1">
              {available.map((name) => (
                <li key={name} className="flex items-center gap-2 py-1 text-sm">
                  <span className="flex-1 truncate">{name}</span>
                  <Button
                    variant="ghost"
                    size="sm"
                    onClick={() => handleAdd(name)}
                    disabled={isDisabled}
                    aria-label={t("settings.sound.prioritizedMicrophones.add")}
                    className="px-1.5 py-0.5 shrink-0"
                  >
                    {"+"}
                  </Button>
                </li>
              ))}
            </ul>
          )}
        </div>
      </div>
    );
  });

PrioritizedMicrophoneSelector.displayName = "PrioritizedMicrophoneSelector";

/**
 * Settings UI for the identifier correction feature.
 *
 * Provides:
 *   - Enable / disable toggle
 *   - Project root path input with re-index button
 *   - Confidence threshold slider
 *   - Live symbol count badge
 */

import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { ToggleSwitch } from "../ui/ToggleSwitch";
import { Input } from "../ui/Input";
import { Button } from "../ui/Button";
import { SettingContainer } from "../ui/SettingContainer";
import { useSettings } from "../../hooks/useSettings";
import { commands } from "@/bindings";

interface IdentifierCorrectionSettingsProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const IdentifierCorrectionSettings: React.FC<
  IdentifierCorrectionSettingsProps
> = React.memo(({ descriptionMode = "tooltip", grouped = false }) => {
  const { t } = useTranslation();
  const { getSetting, updateSetting, isUpdating } = useSettings();

  const enabled =
    (getSetting("identifier_correction_enabled") as boolean) ?? false;
  const projectRoot =
    (getSetting("identifier_correction_project_root") as string | null) ?? "";
  const threshold =
    (getSetting("identifier_correction_threshold") as number) ?? 0.6;

  const [symbolCount, setSymbolCount] = useState<number | null>(null);
  const [indexing, setIndexing] = useState(false);
  const [rootInput, setRootInput] = useState(projectRoot);

  // Keep rootInput in sync with persisted settings (e.g. after a re-render).
  useEffect(() => {
    setRootInput(projectRoot);
  }, [projectRoot]);

  // Fetch the current index size on mount.
  useEffect(() => {
    commands
      .getIdentifierIndexSize()
      .then((n) => setSymbolCount(n))
      .catch(() => {});
  }, []);

  const handleToggle = (value: boolean) => {
    updateSetting("identifier_correction_enabled", value);
  };

  const handleApplyRoot = async () => {
    setIndexing(true);
    try {
      const count = await commands.setIdentifierCorrectionSettings(
        enabled,
        rootInput || null,
        threshold,
      );
      setSymbolCount(count);
      // Persist the root in settings store.
      updateSetting("identifier_correction_project_root", rootInput || null);
    } catch (e) {
      console.error("Failed to build identifier index:", e);
    } finally {
      setIndexing(false);
    }
  };

  const handleRebuild = async () => {
    setIndexing(true);
    try {
      const count = await commands.rebuildIdentifierIndex();
      setSymbolCount(count);
    } catch (e) {
      console.error("Failed to rebuild identifier index:", e);
    } finally {
      setIndexing(false);
    }
  };

  const handleThresholdChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    const val = parseFloat(e.target.value);
    updateSetting("identifier_correction_threshold", val);
  };

  return (
    <>
      {/* Enable toggle */}
      <ToggleSwitch
        checked={enabled}
        onChange={handleToggle}
        isUpdating={isUpdating("identifier_correction_enabled")}
        label={t("settings.advanced.identifierCorrection.title")}
        description={t("settings.advanced.identifierCorrection.description")}
        descriptionMode={descriptionMode}
        grouped={grouped}
      />

      {/* Project root path + index controls */}
      {enabled && (
        <SettingContainer
          title={t("settings.advanced.identifierCorrection.projectRoot")}
          description={t(
            "settings.advanced.identifierCorrection.projectRootDescription",
          )}
          descriptionMode={descriptionMode}
          grouped={grouped}
          layout="stacked"
        >
          <div className="flex items-center gap-2 mt-1">
            <Input
              type="text"
              className="flex-1 font-mono text-xs"
              value={rootInput}
              onChange={(e) => setRootInput(e.target.value)}
              placeholder={t(
                "settings.advanced.identifierCorrection.projectRootPlaceholder",
              )}
              variant="compact"
            />
            <Button
              onClick={handleApplyRoot}
              disabled={indexing || !rootInput.trim()}
              variant="primary"
              size="md"
            >
              {indexing
                ? t("settings.advanced.identifierCorrection.indexing")
                : t("settings.advanced.identifierCorrection.index")}
            </Button>
            {symbolCount !== null && symbolCount > 0 && (
              <Button
                onClick={handleRebuild}
                disabled={indexing}
                variant="secondary"
                size="md"
              >
                {t("settings.advanced.identifierCorrection.refresh")}
              </Button>
            )}
          </div>

          {/* Symbol count badge */}
          {symbolCount !== null && (
            <p className="text-xs text-mid-gray mt-1.5">
              {symbolCount === 0
                ? t("settings.advanced.identifierCorrection.noSymbols")
                : t("settings.advanced.identifierCorrection.symbolCount", {
                    count: symbolCount,
                  })}
            </p>
          )}
        </SettingContainer>
      )}

      {/* Confidence threshold slider */}
      {enabled && symbolCount !== null && symbolCount > 0 && (
        <SettingContainer
          title={t("settings.advanced.identifierCorrection.threshold")}
          description={t(
            "settings.advanced.identifierCorrection.thresholdDescription",
          )}
          descriptionMode={descriptionMode}
          grouped={grouped}
        >
          <div className="flex items-center gap-3">
            <input
              type="range"
              min="0.3"
              max="0.95"
              step="0.05"
              value={threshold}
              onChange={handleThresholdChange}
              className="w-28 accent-logo-primary"
              disabled={isUpdating("identifier_correction_threshold")}
            />
            <span className="text-xs tabular-nums text-mid-gray w-8">
              {threshold.toFixed(2)}
            </span>
          </div>
        </SettingContainer>
      )}
    </>
  );
});

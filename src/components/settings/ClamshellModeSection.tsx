import React, { useState, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { ChevronDown } from "lucide-react";
import { commands } from "@/bindings";
import { Dropdown } from "../ui/Dropdown";
import { ToggleSwitch } from "../ui/ToggleSwitch";
import { SettingContainer } from "../ui/SettingContainer";
import { useSettings } from "../../hooks/useSettings";

// macOS built-in microphones are named e.g. "MacBook Pro Microphone",
// "MacBook Air Microphone", or "Built-in Microphone".
const BUILTIN_MIC_RE = /macbook.*microphone|built-?in microphone/i;
const isBuiltinMicName = (name?: string | null): boolean =>
  !!name && BUILTIN_MIC_RE.test(name);

// "Default"/None means "no fallback configured" (the backend treats it as disabled).
const DISABLED_VALUES = ["Default", "default", ""];

interface ClamshellModeSectionProps {
  grouped?: boolean;
}

/**
 * Collapsible "Clamshell mode" control shown in the Sound settings on laptops.
 * Lets the user opt into falling back to a different microphone while in
 * clamshell mode (lid closed + external display), where the built-in mic is
 * physically muffled.
 *
 * Backed entirely by the existing `clamshell_microphone` setting:
 *   - "Default"/None  -> fallback disabled
 *   - a device name   -> fallback enabled, use that device
 * The audio manager already swaps to it when `is_clamshell()` is true.
 */
export const ClamshellModeSection: React.FC<ClamshellModeSectionProps> =
  React.memo(({ grouped = true }) => {
    const { t } = useTranslation();
    const {
      getSetting,
      updateSetting,
      isUpdating,
      isLoading,
      audioDevices,
      refreshAudioDevices,
    } = useSettings();

    const [isLaptop, setIsLaptop] = useState<boolean>(false);
    const [expanded, setExpanded] = useState<boolean>(false);

    useEffect(() => {
      commands
        .isLaptop()
        .then((result) =>
          setIsLaptop(result.status === "ok" ? result.data : false),
        )
        .catch((error) => {
          console.error("Failed to check if device is laptop:", error);
          setIsLaptop(false);
        });
    }, []);

    // Clamshell mode only makes sense on a laptop (the lid-closed scenario),
    // and clamshell detection is macOS-only — so is_laptop() (false on non-macOS)
    // is the right gate, matching the old Debug-tab picker exactly. Shown
    // regardless of which mic is selected; only the built-in-mic gate is dropped.
    if (!isLaptop) {
      return null;
    }

    const clamshellMicrophone = getSetting("clamshell_microphone");
    const enabled =
      !!clamshellMicrophone && !DISABLED_VALUES.includes(clamshellMicrophone);

    // Candidate fallback mics: real devices other than the built-in (and not the
    // synthetic "Default" entry, which would map back to "disabled").
    const fallbackOptions = audioDevices
      .filter((d) => d.name !== "Default" && !isBuiltinMicName(d.name))
      .map((d) => ({ value: d.name, label: d.name }));
    const noFallbackAvailable = fallbackOptions.length === 0;

    const handleToggle = async (on: boolean) => {
      if (on) {
        await updateSetting(
          "clamshell_microphone",
          fallbackOptions[0]?.value ?? "Default",
        );
      } else {
        await updateSetting("clamshell_microphone", "Default");
      }
    };

    const handleSelect = async (deviceName: string) => {
      await updateSetting("clamshell_microphone", deviceName);
    };

    return (
      <div className={grouped ? "" : "rounded-lg border border-mid-gray/20"}>
        <button
          type="button"
          onClick={() => setExpanded((v) => !v)}
          aria-expanded={expanded}
          className="w-full flex items-center justify-between px-4 p-2 hover:bg-white/5 transition-colors"
        >
          <span className="text-sm font-medium">
            {t("settings.sound.clamshell.title")}
          </span>
          <ChevronDown
            className={`w-4 h-4 text-mid-gray transition-transform duration-200 ${
              expanded ? "rotate-180" : ""
            }`}
          />
        </button>

        {expanded && (
          <div className="pb-1 ml-4 border-l border-mid-gray/20">
            <ToggleSwitch
              checked={enabled}
              onChange={handleToggle}
              // Block turning the fallback ON when there's nothing to pick, but
              // never trap an already-enabled setting (e.g. the chosen mic was
              // unplugged) — the user must always be able to turn it back off.
              disabled={!enabled && noFallbackAvailable}
              isUpdating={isUpdating("clamshell_microphone")}
              label={t("settings.sound.clamshell.enable.label")}
              description={t("settings.sound.clamshell.enable.description")}
              descriptionMode="tooltip"
              grouped={true}
            />

            {noFallbackAvailable && (
              <p className="px-4 pb-2 text-xs text-mid-gray">
                {t("settings.sound.clamshell.noFallback")}
              </p>
            )}

            {enabled && !noFallbackAvailable && (
              <SettingContainer
                title={t("settings.sound.clamshell.device.title")}
                description={t("settings.sound.clamshell.device.description")}
                descriptionMode="tooltip"
                grouped={true}
              >
                <div className="flex items-center space-x-1">
                  <Dropdown
                    options={fallbackOptions}
                    selectedValue={clamshellMicrophone ?? ""}
                    onSelect={handleSelect}
                    placeholder={
                      isLoading
                        ? t("settings.sound.microphone.loading")
                        : t("settings.sound.microphone.placeholder")
                    }
                    disabled={isUpdating("clamshell_microphone") || isLoading}
                    onRefresh={refreshAudioDevices}
                  />
                </div>
              </SettingContainer>
            )}
          </div>
        )}
      </div>
    );
  });

ClamshellModeSection.displayName = "ClamshellModeSection";

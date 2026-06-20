import React from "react";
import { useTranslation } from "react-i18next";
import { Dropdown } from "../ui/Dropdown";
import { SettingContainer } from "../ui/SettingContainer";
import { Input } from "../ui/Input";
import { Alert } from "../ui/Alert";
import { useSettings } from "../../hooks/useSettings";
import { useOsType } from "../../hooks/useOsType";
import type { PasteMethod } from "@/bindings";

interface PasteMethodProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const PasteMethodSetting: React.FC<PasteMethodProps> = React.memo(
  ({ descriptionMode = "tooltip", grouped = false }) => {
    const { t } = useTranslation();
    const { getSetting, updateSetting, isUpdating, getSettingError } =
      useSettings();
    const osType = useOsType();

    const getPasteMethodOptions = (osType: string) => {
      const mod = osType === "macos" ? "Cmd" : "Ctrl";

      const options = [
        {
          value: "ctrl_v",
          label: t("settings.advanced.pasteMethod.options.clipboard", {
            modifier: mod,
          }),
        },
        {
          value: "direct",
          label: t("settings.advanced.pasteMethod.options.direct"),
        },
        {
          value: "none",
          label: t("settings.advanced.pasteMethod.options.none"),
        },
      ];

      // Add Shift+Insert and Ctrl+Shift+V options for Windows and Linux only
      if (osType === "windows" || osType === "linux") {
        options.push(
          {
            value: "ctrl_shift_v",
            label: t(
              "settings.advanced.pasteMethod.options.clipboardCtrlShiftV",
            ),
          },
          {
            value: "shift_insert",
            label: t(
              "settings.advanced.pasteMethod.options.clipboardShiftInsert",
            ),
          },
        );
      }

      // External script is only available on Linux
      if (osType === "linux") {
        options.push(
          {
            value: "external_script",
            label: t("settings.advanced.pasteMethod.options.externalScript"),
          },
          {
            value: "capglue",
            label: t("settings.advanced.pasteMethod.options.capglue"),
          },
        );
      }

      return options;
    };

    const selectedMethod = (getSetting("paste_method") ||
      "ctrl_v") as PasteMethod;
    const externalScriptPath = getSetting("external_script_path") || "";
    const capglueSettings = getSetting("capglue_settings") || {
      target: "",
      command: "capglue",
      args: [],
    };
    const capglueError = getSettingError("capglue_settings");

    const updateCapglueSettings = (
      updates: Partial<typeof capglueSettings>,
    ) => {
      updateSetting("capglue_settings", {
        ...capglueSettings,
        ...updates,
      });
    };

    const pasteMethodOptions = getPasteMethodOptions(osType);

    return (
      <SettingContainer
        title={t("settings.advanced.pasteMethod.title")}
        description={t("settings.advanced.pasteMethod.description")}
        descriptionMode={descriptionMode}
        grouped={grouped}
        tooltipPosition="bottom"
      >
        <div className="flex flex-col gap-2">
          <Dropdown
            options={pasteMethodOptions}
            selectedValue={selectedMethod}
            onSelect={(value) =>
              updateSetting("paste_method", value as PasteMethod)
            }
            disabled={isUpdating("paste_method")}
          />
          {selectedMethod === "external_script" && (
            <Input
              type="text"
              value={externalScriptPath}
              onChange={(e) =>
                updateSetting("external_script_path", e.target.value)
              }
              placeholder={t(
                "settings.advanced.pasteMethod.externalScriptPlaceholder",
              )}
              disabled={isUpdating("external_script_path")}
            />
          )}
          {selectedMethod === "capglue" && (
            <div className="flex flex-col gap-2">
              <p className="text-xs text-mid-gray/70">
                {capglueSettings.target
                  ? t("settings.advanced.pasteMethod.capglueHelp")
                  : t("settings.advanced.pasteMethod.capglueUnavailable")}
              </p>
              {capglueError && (
                <Alert variant="error" contained>
                  {capglueError}
                </Alert>
              )}
              <Input
                type="text"
                value={capglueSettings.target}
                onChange={(e) =>
                  updateCapglueSettings({ target: e.target.value })
                }
                placeholder={t(
                  "settings.advanced.pasteMethod.capglueTargetPlaceholder",
                )}
                disabled={isUpdating("capglue_settings")}
              />
              <Input
                type="text"
                value={capglueSettings.command}
                onChange={(e) =>
                  updateCapglueSettings({ command: e.target.value })
                }
                placeholder={t(
                  "settings.advanced.pasteMethod.capglueCommandPlaceholder",
                )}
                disabled={isUpdating("capglue_settings")}
              />
            </div>
          )}
        </div>
      </SettingContainer>
    );
  },
);

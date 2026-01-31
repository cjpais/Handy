import React, { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { listen } from "@tauri-apps/api/event";
import { Dropdown } from "../ui/Dropdown";
import { SettingContainer } from "../ui/SettingContainer";
import { Alert } from "../ui/Alert";
import { Button } from "../ui/Button";
import { useSettings } from "../../hooks/useSettings";
import { useOsType } from "../../hooks/useOsType";
import { useWayland } from "../../hooks/useWayland";
import { commands } from "@/bindings";
import type { PasteMethod } from "@/bindings";

interface PasteMethodProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const PasteMethodSetting: React.FC<PasteMethodProps> = React.memo(
  ({ descriptionMode = "tooltip", grouped = false }) => {
    const { t } = useTranslation();
    const { getSetting, updateSetting, isUpdating } = useSettings();
    const osType = useOsType();
    const isWayland = useWayland();
    const [isRDRequesting, setIsRDRequesting] = useState(false);
    const [isRDAuthorized, setIsRDAuthorized] = useState(false);

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

      return options;
    };

    const selectedMethod = (getSetting("paste_method") ||
      "ctrl_v") as PasteMethod;

    const pasteMethodOptions = getPasteMethodOptions(osType);

    // =========================================================================
    // Remote Desktop Authorization State (Wayland only)
    // =========================================================================
    // Helper: fetch remote desktop authorization state.
    const fetchRDAuthorization = async () => {
      try {
        const authorized = await commands.getRemoteDesktopAuthorization();
        setIsRDAuthorized(authorized);
      } catch {
        setIsRDAuthorized(false);
      }
    };

    // Only Wayland (linux)
    // Init value for isRDAuthorized
    // And listen if there is any change from the backend
    useEffect(() => {
      if (!isWayland) return;
      // Fetch for the initial state.
      fetchRDAuthorization();
      // Listen for updates on any changes.
      let unlisten: (() => void) | null = null;
      listen<boolean>("remote-desktop-auth-changed", (event) => {
        setIsRDAuthorized(Boolean(event.payload));
      }).then((stop) => {
        unlisten = stop;
      });
      return () => {
        if (unlisten) unlisten();
      };
    }, [isWayland]);

    const shouldShowRDRequest = isWayland && selectedMethod === "direct";

    const handleRDRequest = async () => {
      if (isRDRequesting) return;
      setIsRDRequesting(true);
      const result = await commands.requestRemoteDesktopAuthorization();
      if (result.status === "error") {
        toast.error(
          t("settings.advanced.pasteMethod.portal.errors.requestFailed"),
        );
      }
      setIsRDRequesting(false);
    };

    const handleRDRevoke = async () => {
      const result = await commands.deleteRemoteDesktopAuthorization();
      if (result.status === "error") {
        toast.error(
          t("settings.advanced.pasteMethod.portal.errors.revokeFailed"),
        );
      }
    };

    return (
      <div>
        {isRDRequesting && (
          <div className="fixed inset-0 z-[1000] bg-black/30 backdrop-blur-sm cursor-wait flex items-center justify-center pointer-events-auto">
            <div className="rounded-md bg-neutral-900/85 px-4 py-3 text-sm text-white shadow-lg">
              {t("settings.advanced.pasteMethod.portal.buttonRequesting")}
            </div>
          </div>
        )}
        <SettingContainer
          title={t("settings.advanced.pasteMethod.title")}
          description={t("settings.advanced.pasteMethod.description")}
          descriptionMode={descriptionMode}
          grouped={grouped}
          tooltipPosition="bottom"
        >
          <div className="flex flex-col gap-3">
            <Dropdown
              options={pasteMethodOptions}
              selectedValue={selectedMethod}
              onSelect={async (value) => {
                if (value === selectedMethod) return;

                // Update the paste method setting, then run any side effects we manage here.
                await updateSetting("paste_method", value as PasteMethod);

                // If the user switches to the direct method on Linux/Wayland and the
                // If Remote Desktop permission is active, it is revoked
                if (isRDAuthorized && shouldShowRDRequest) {
                  await commands.deleteRemoteDesktopAuthorization();
                }
              }}
              disabled={isUpdating("paste_method")}
            />
          </div>
        </SettingContainer>
        {shouldShowRDRequest && (
          <div className="mr-4 ml-4 mb-4 mt-4">
            <Alert
              variant={isRDAuthorized ? "info" : "warning"}
              contained={true}
            >
              {isRDAuthorized ? (
                <div>
                  <div>
                    {t("settings.advanced.pasteMethod.portal.authorized")}
                  </div>
                  <div className="italic">
                    {t("settings.advanced.pasteMethod.portal.authorizedRappel")}
                  </div>
                </div>
              ) : (
                t("settings.advanced.pasteMethod.portal.description")
              )}
              <div className="justify-center mt-4">
                <Button
                  variant={isRDAuthorized ? "secondary" : "primary"}
                  size="sm"
                  onClick={isRDAuthorized ? handleRDRevoke : handleRDRequest}
                  disabled={isRDRequesting}
                >
                  {isRDAuthorized
                    ? t("settings.advanced.pasteMethod.portal.buttonRevoke")
                    : isRDRequesting
                      ? t(
                          "settings.advanced.pasteMethod.portal.buttonRequesting",
                        )
                      : t("settings.advanced.pasteMethod.portal.button")}
                </Button>
              </div>
            </Alert>
          </div>
        )}
      </div>
    );
  },
);

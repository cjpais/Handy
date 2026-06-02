import React, { useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { CheckCircle } from "lucide-react";
import { SettingContainer } from "../ui/SettingContainer";
import { useRemoteDesktopAuthorization } from "../../hooks/useRemoteDesktopAuthorization";
import { commands } from "@/bindings";
import type { PasteMethod, TypingTool } from "@/bindings";

interface RemoteDesktopAuthorizationCardProps {
  pasteMethod: PasteMethod;
  typingTool: TypingTool;
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

/**
 * Displays the Remote Desktop portal authorization control when direct Wayland
 * typing depends on it.
 *
 * Inputs: current paste method, typing tool, and settings row display options.
 * Outputs: a settings row with authorization status and an enable/disable toggle.
 * Side effects: requests or revokes the portal token through Tauri commands.
 */
export const RemoteDesktopAuthorizationCard: React.FC<RemoteDesktopAuthorizationCardProps> =
  React.memo(
    ({
      pasteMethod,
      typingTool,
      descriptionMode = "tooltip",
      grouped = false,
    }) => {
      const { t } = useTranslation();
      const { isRelevant, isAuthorized, setIsAuthorized } =
        useRemoteDesktopAuthorization(pasteMethod, typingTool);
      const [pendingAction, setPendingAction] = useState<
        "request" | "revoke" | null
      >(null);

      const handleRequestAuthorization = async () => {
        if (pendingAction) return;

        setPendingAction("request");
        try {
          const result = await commands.requestRemoteDesktopAuthorization();
          if (result.status === "error") {
            toast.error(
              t("settings.advanced.pasteMethod.portal.errors.requestFailed"),
            );
          } else if (result.data) {
            setIsAuthorized(true);
          }
        } catch {
          toast.error(
            t("settings.advanced.pasteMethod.portal.errors.requestFailed"),
          );
        } finally {
          setPendingAction(null);
        }
      };

      const handleRevokeAuthorization = async () => {
        if (pendingAction) return;

        setPendingAction("revoke");
        try {
          const result = await commands.deleteRemoteDesktopAuthorization();
          if (result.status === "error") {
            toast.error(
              t("settings.advanced.pasteMethod.portal.errors.revokeFailed"),
            );
            return;
          }

          setIsAuthorized(false);
        } catch {
          toast.error(
            t("settings.advanced.pasteMethod.portal.errors.revokeFailed"),
          );
        } finally {
          setPendingAction(null);
        }
      };

      const handleAuthorizationToggle = (enabled: boolean) => {
        if (enabled) {
          void handleRequestAuthorization();
          return;
        }

        void handleRevokeAuthorization();
      };

      if (!isRelevant) {
        return null;
      }

      return (
        <SettingContainer
          title={t("onboarding.permissions.remoteDesktop.title")}
          description={`${t("onboarding.permissions.remoteDesktop.description")} ${t(
            "onboarding.permissions.remoteDesktop.note",
          )}`}
          descriptionMode={descriptionMode}
          grouped={grouped}
          tooltipPosition="bottom"
        >
          <div className="grid min-w-[200px] grid-cols-[1fr_auto] items-center gap-3">
            <div className="justify-self-start">
              {pendingAction === "request" ? (
                <span className="text-xs font-medium text-yellow-500">
                  {t("settings.advanced.pasteMethod.portal.buttonRequesting")}
                </span>
              ) : isAuthorized ? (
                <span className="flex items-center gap-1.5 text-xs font-medium text-logo-primary">
                  <CheckCircle className="w-3.5 h-3.5" />
                  {t("onboarding.permissions.remoteDesktop.configured")}
                </span>
              ) : (
                <span className="text-xs font-medium text-yellow-500">
                  {t("onboarding.permissions.remoteDesktop.notConfigured")}
                </span>
              )}
            </div>
            <label
              className={`inline-flex items-center justify-self-end ${
                pendingAction ? "cursor-not-allowed" : "cursor-pointer"
              }`}
            >
              <input
                type="checkbox"
                className="sr-only peer"
                checked={isAuthorized}
                disabled={pendingAction !== null}
                aria-label={t("onboarding.permissions.remoteDesktop.title")}
                onChange={(event) =>
                  handleAuthorizationToggle(event.target.checked)
                }
              />
              <div className="relative w-11 h-6 bg-mid-gray/20 peer-focus:outline-none peer-focus:ring-4 peer-focus:ring-logo-primary rounded-full peer peer-checked:after:translate-x-full rtl:peer-checked:after:-translate-x-full peer-checked:after:border-white after:content-[''] after:absolute after:top-[2px] after:start-[2px] after:bg-white after:border-gray-300 after:border after:rounded-full after:h-5 after:w-5 after:transition-all peer-checked:bg-background-ui peer-disabled:opacity-50"></div>
            </label>
          </div>
        </SettingContainer>
      );
    },
  );

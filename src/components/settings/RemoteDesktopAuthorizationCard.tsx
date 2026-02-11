import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { listen } from "@tauri-apps/api/event";
import { Alert } from "../ui/Alert";
import { Button } from "../ui/Button";
import { useWayland } from "../../hooks/useWayland";
import { commands } from "@/bindings";
import type { TypingTool } from "@/bindings";

interface RemoteDesktopAuthorizationCardProps {
  typingTool: TypingTool;
}

const isRemoteDesktopRelevantTool = (typingTool: TypingTool): boolean =>
  typingTool === "auto" || typingTool === "remote_desktop";

export const RemoteDesktopAuthorizationCard: React.FC<
  RemoteDesktopAuthorizationCardProps
> = React.memo(({ typingTool }) => {
  const { t } = useTranslation();
  const isWayland = useWayland();
  const isToolRelevant = isRemoteDesktopRelevantTool(typingTool);
  const isCardVisible = isWayland && isToolRelevant;
  const [isRDRequesting, setIsRDRequesting] = useState(false);
  const [isRDAuthorized, setIsRDAuthorized] = useState(false);
  // Keep authorization state synced only while the card is visible.
  useEffect(() => {
    if (!isCardVisible) return;

    const fetchRDAuthorization = async () => {
      try {
        const authorized = await commands.getRemoteDesktopAuthorization();
        setIsRDAuthorized(authorized);
      } catch {
        setIsRDAuthorized(false);
      }
    };

    fetchRDAuthorization();

    let unlisten: (() => void) | null = null;
    listen<boolean>("remote-desktop-auth-changed", (event) => {
      setIsRDAuthorized(Boolean(event.payload));
    }).then((stop) => {
      unlisten = stop;
    });

    return () => {
      if (unlisten) unlisten();
    };
  }, [isCardVisible]);

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

  if (!isCardVisible) {
    return null;
  }

  return (
    <>
      {isRDRequesting && (
        <div className="fixed inset-0 z-[1000] bg-black/30 backdrop-blur-sm cursor-wait flex items-center justify-center pointer-events-auto">
          <div className="rounded-md bg-neutral-900/85 px-4 py-3 text-sm text-white shadow-lg">
            {t("settings.advanced.pasteMethod.portal.buttonRequesting")}
          </div>
        </div>
      )}
      <div className="mr-4 ml-4 mb-4 mt-4">
        <Alert variant={isRDAuthorized ? "info" : "warning"} contained={true}>
          {isRDAuthorized ? (
            <div>
              <div>{t("settings.advanced.pasteMethod.portal.authorized")}</div>
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
                  ? t("settings.advanced.pasteMethod.portal.buttonRequesting")
                  : t("settings.advanced.pasteMethod.portal.button")}
            </Button>
          </div>
        </Alert>
      </div>
    </>
  );
});

import React from "react";
import { useTranslation } from "react-i18next";
import { AlertTriangle } from "lucide-react";
import { Tooltip } from "../ui/Tooltip";

/** Displays the portal authorization warning using the shared tooltip. */
export const RemoteDesktopAuthorizationWarning: React.FC = () => {
  const { t } = useTranslation();
  const warning = t("onboarding.permissions.remoteDesktop.warningTooltip");

  return (
    <Tooltip text={warning} placement="below" showOnPress>
      <button
        type="button"
        aria-label={warning}
        className="flex cursor-help items-center text-yellow-500"
      >
        <AlertTriangle className="w-4 h-4" aria-hidden="true" />
      </button>
    </Tooltip>
  );
};

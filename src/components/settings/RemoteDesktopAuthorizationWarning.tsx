import React, { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { AlertTriangle } from "lucide-react";
import { Tooltip } from "../ui/Tooltip";

/**
 * Shows a compact warning for settings that need automatic typing authorization.
 *
 * Inputs: none.
 * Outputs: a warning icon with a localized tooltip.
 * Side effects: listens for outside clicks to dismiss the tooltip.
 */
export const RemoteDesktopAuthorizationWarning: React.FC = () => {
  const { t } = useTranslation();
  const [showTooltip, setShowTooltip] = useState(false);
  const tooltipRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const handleClickOutside = (event: MouseEvent) => {
      if (
        tooltipRef.current &&
        !tooltipRef.current.contains(event.target as Node)
      ) {
        setShowTooltip(false);
      }
    };

    if (showTooltip) {
      document.addEventListener("mousedown", handleClickOutside);
      return () =>
        document.removeEventListener("mousedown", handleClickOutside);
    }
  }, [showTooltip]);

  const toggleTooltip = () => {
    setShowTooltip((visible) => !visible);
  };

  return (
    <div
      ref={tooltipRef}
      className="relative flex items-center"
      onMouseEnter={() => setShowTooltip(true)}
      onMouseLeave={() => setShowTooltip(false)}
      onClick={toggleTooltip}
    >
      <AlertTriangle
        className="w-4 h-4 text-yellow-500 cursor-help"
        aria-label={t("onboarding.permissions.remoteDesktop.warningTooltip")}
        role="button"
        tabIndex={0}
        onKeyDown={(event) => {
          if (event.key === "Enter" || event.key === " ") {
            event.preventDefault();
            toggleTooltip();
          }
        }}
      />
      {showTooltip && (
        <Tooltip targetRef={tooltipRef} position="bottom">
          <p className="text-sm text-center leading-relaxed">
            {t("onboarding.permissions.remoteDesktop.warningTooltip")}
          </p>
        </Tooltip>
      )}
    </div>
  );
};

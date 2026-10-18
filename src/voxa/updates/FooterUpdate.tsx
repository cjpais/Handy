import React from "react";
import { useTranslation } from "react-i18next";
import { availableVersion, useUpdateStore } from "./updateStore";
import { OPEN_UPDATES_DOM_EVENT } from "./useUpdatesBridge";

/**
 * Footer hint: stays empty until a version is waiting, then offers a link to
 * the updates page. The check itself runs in the backend.
 */
export const FooterUpdate: React.FC = () => {
  const { t } = useTranslation();
  const version = availableVersion(useUpdateStore((s) => s.status));
  if (!version) return null;
  return (
    <>
      <button
        type="button"
        className="text-logo-primary hover:text-logo-primary/80 font-medium transition-colors"
        onClick={() => window.dispatchEvent(new Event(OPEN_UPDATES_DOM_EVENT))}
        data-testid="footer-update"
      >
        {t("voxa.updates.footerAvailable", { version })}
      </button>
      <span>{"•"}</span>
    </>
  );
};

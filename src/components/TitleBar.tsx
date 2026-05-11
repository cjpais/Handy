import { getCurrentWindow } from "@tauri-apps/api/window";
import { Minus, Square, X } from "lucide-react";
import { useTranslation } from "react-i18next";

const currentWindow = getCurrentWindow();

const buttonClassName =
  "flex h-8 w-10 items-center justify-center text-mid-gray transition-colors hover:bg-mid-gray/10 focus:outline-none focus:bg-mid-gray/20";

const TitleBar = () => {
  const { t } = useTranslation();

  const minimizeWindow = () => {
    void currentWindow.minimize().catch((error) => {
      console.error("Failed to minimize window:", error);
    });
  };

  const toggleMaximizeWindow = () => {
    void currentWindow.toggleMaximize().catch((error) => {
      console.error("Failed to toggle maximize:", error);
    });
  };

  const closeWindow = () => {
    void currentWindow.close().catch((error) => {
      console.error("Failed to close window:", error);
    });
  };

  return (
    <div className="flex h-8 shrink-0 items-center justify-between border-b border-mid-gray/20 bg-background">
      <div
        data-tauri-drag-region
        className="flex h-full min-w-0 flex-1 items-center px-3 text-xs font-medium uppercase text-mid-gray/80"
      >
        {t("common.appName")}
      </div>
      <div className="flex h-full items-center">
        <button
          aria-label={t("windowControls.minimize")}
          className={buttonClassName}
          onClick={minimizeWindow}
          type="button"
        >
          <Minus size={14} />
        </button>
        <button
          aria-label={t("windowControls.maximize")}
          className={buttonClassName}
          onClick={toggleMaximizeWindow}
          type="button"
        >
          <Square size={13} />
        </button>
        <button
          aria-label={t("windowControls.close")}
          className={`${buttonClassName} hover:bg-red-500 hover:text-white focus:bg-red-500 focus:text-white`}
          onClick={closeWindow}
          type="button"
        >
          <X size={15} />
        </button>
      </div>
    </div>
  );
};

export default TitleBar;

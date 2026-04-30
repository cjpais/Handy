import { getCurrentWindow } from "@tauri-apps/api/window";
import { Minus, Square, X } from "lucide-react";
import { useTranslation } from "react-i18next";

const win = getCurrentWindow();

const buttonClassName =
  "flex h-8 w-10 items-center justify-center text-mid-gray transition-colors hover:bg-mid-gray/10";

const TitleBar = () => {
  const { t } = useTranslation();

  const handleMinimize = () => {
    void win.minimize().catch((error) => {
      console.error("Failed to minimize window", error);
    });
  };

  const handleToggleMaximize = () => {
    void win.toggleMaximize().catch((error) => {
      console.error("Failed to toggle maximize", error);
    });
  };

  const handleClose = () => {
    void win.close().catch((error) => {
      console.error("Failed to close window", error);
    });
  };

  return (
    <div className="flex h-8 shrink-0 items-center justify-between border-b border-mid-gray/10 bg-background">
      <div
        data-tauri-drag-region
        className="flex h-full min-w-0 flex-1 items-center px-3 text-xs font-medium uppercase tracking-[0.2em] text-mid-gray/80"
      >
        {t("common.appName")}
      </div>
      <div className="flex items-center">
        <button
          aria-label="Minimize window"
          onClick={handleMinimize}
          className={buttonClassName}
          type="button"
        >
          <Minus size={14} />
        </button>
        <button
          aria-label="Maximize window"
          onClick={handleToggleMaximize}
          className={buttonClassName}
          type="button"
        >
          <Square size={14} />
        </button>
        <button
          aria-label="Close window"
          onClick={handleClose}
          className={`${buttonClassName} hover:bg-red-500 hover:text-white`}
          type="button"
        >
          <X size={14} />
        </button>
      </div>
    </div>
  );
};

export default TitleBar;

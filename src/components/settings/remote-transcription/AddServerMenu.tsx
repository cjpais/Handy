import React, { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Plus } from "lucide-react";
import { Button } from "../../ui/Button";
import { REMOTE_SERVER_PRESETS } from "@/lib/remoteServers";
import { K, usePresetName } from "./shared";

/** "Add server": a button that opens the presets, not a second picker. */
export const AddServerMenu: React.FC<{
  onPick: (presetKey: string) => void;
}> = ({ onPick }) => {
  const { t } = useTranslation();
  const presetName = usePresetName();
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!open) return;
    const outside = (e: MouseEvent) => {
      if (!ref.current?.contains(e.target as Node)) setOpen(false);
    };
    const escape = (e: KeyboardEvent) => e.key === "Escape" && setOpen(false);
    document.addEventListener("mousedown", outside);
    document.addEventListener("keydown", escape);
    return () => {
      document.removeEventListener("mousedown", outside);
      document.removeEventListener("keydown", escape);
    };
  }, [open]);

  return (
    <div ref={ref} className="relative shrink-0">
      <Button
        variant="secondary"
        size="sm"
        aria-haspopup="menu"
        aria-expanded={open}
        onClick={() => setOpen((o) => !o)}
      >
        <span className="flex items-center gap-1">
          <Plus className="size-[13px]" aria-hidden="true" />
          {t(`${K}.add`)}
        </span>
      </Button>
      {open && (
        <div
          role="menu"
          className="absolute end-0 top-full z-50 mt-1 max-h-[340px] w-72 overflow-y-auto rounded-lg border border-line bg-background p-1 shadow-lg"
        >
          {REMOTE_SERVER_PRESETS.map((preset) => (
            <button
              key={preset.key}
              type="button"
              role="menuitem"
              onClick={() => {
                setOpen(false);
                onPick(preset.key);
              }}
              className="flex w-full cursor-pointer flex-col items-start rounded-md px-2 py-1.5 text-start hover:bg-wash focus:bg-wash focus:outline-none"
            >
              <span className="text-sm">
                {presetName(preset.key, preset.name)}
              </span>
              <span className="text-xs text-ink-3">
                {t(`${K}.presets.hints.${preset.key}`)}
              </span>
            </button>
          ))}
        </div>
      )}
    </div>
  );
};

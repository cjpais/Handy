import React, { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { CircleHelp } from "lucide-react";
import { GradeChip } from "@/components/shared/GradeChip";
import { SpeedBars } from "@/components/shared/SpeedBars";
import { useRubric } from "@/components/shared/modelTable/rubric";
import { usePressAway } from "@/hooks/usePressAway";

const LETTERS = ["A", "B", "C", "D", "F"] as const;

/** The "?" in the column heads: opens the full grade and speed scale. */
export const Legend: React.FC<{ tier: "accelerated" | "cpu" }> = ({ tier }) => {
  const { t } = useTranslation();
  const rubric = useRubric();
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);

  usePressAway(open, ref, () => setOpen(false));
  useEffect(() => {
    if (!open) return;
    const esc = (e: KeyboardEvent) => e.key === "Escape" && setOpen(false);
    document.addEventListener("keydown", esc);
    return () => document.removeEventListener("keydown", esc);
  }, [open]);

  return (
    <div ref={ref} className="relative grid">
      <button
        type="button"
        aria-expanded={open}
        aria-label={t("settings.models.legend.open")}
        title={t("settings.models.legend.open")}
        onClick={() => setOpen((o) => !o)}
        className={`grid size-[22px] cursor-pointer place-items-center rounded-full transition-colors focus:outline-none focus-visible:ring-1 focus-visible:ring-logo-primary ${open ? "bg-line text-text" : "text-ink-3 hover:bg-wash hover:text-text"}`}
      >
        <CircleHelp className="size-[15px]" aria-hidden="true" />
      </button>
      {open && (
        <div
          role="dialog"
          aria-label={t("settings.models.legend.open")}
          className="absolute end-0 top-[28px] z-30 w-[330px] animate-rise rounded-card border border-line-strong bg-background p-[14px] text-start text-small leading-[1.45] shadow-[0_12px_32px_-12px_rgba(0,0,0,0.35)]"
        >
          <p className="font-semibold text-text">{t("modelTable.accuracy")}</p>
          <p className="mt-[2px] text-ink-2">
            {t("settings.models.legend.accuracyNote")}
          </p>
          <ul className="mt-[8px] grid grid-cols-[auto_1fr_auto_1fr] items-center gap-x-[8px] gap-y-[5px]">
            {LETTERS.map((l) => (
              <React.Fragment key={l}>
                <GradeChip letter={l} label={l} />
                <span className="text-ink-2 tabular-nums">
                  {rubric.gradeBand(l)}
                </span>
              </React.Fragment>
            ))}
          </ul>
          <p className="mt-[12px] font-semibold text-text">
            {t("modelTable.speed")}
          </p>
          <p className="mt-[2px] text-ink-2">
            {t(`settings.models.legend.speedNote.${tier}`)}
          </p>
          <ul className="mt-[8px] grid grid-cols-[auto_1fr_auto_1fr] items-center gap-x-[8px] gap-y-[5px]">
            {[4, 3, 2, 1].map((level) => (
              <React.Fragment key={level}>
                <span className="inline-grid w-[30px] place-items-center">
                  <SpeedBars level={level} />
                </span>
                <span className="text-ink-2 tabular-nums">
                  {rubric.speedBand(level)}
                </span>
              </React.Fragment>
            ))}
          </ul>
          <p className="mt-[10px] border-t border-line pt-[8px] text-ink-3">
            {t("settings.models.legend.marks")}
          </p>
        </div>
      )}
    </div>
  );
};

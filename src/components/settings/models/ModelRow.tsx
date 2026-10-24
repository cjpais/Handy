import React, { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  ArrowDown,
  Languages,
  Loader2,
  Radio as LiveIcon,
  Trash2,
  X,
} from "lucide-react";
import { Radio } from "@/components/ui/Radio";
import { Button } from "@/components/ui/Button";
import { Tooltip } from "@/components/ui/Tooltip";
import { GradeChip } from "@/components/shared/GradeChip";
import { ProgressRing } from "@/components/shared/ProgressRing";
import { SpeedBars, speedLevel } from "@/components/shared/SpeedBars";
import { Tag } from "@/components/shared/Tag";
import { useRubric } from "@/components/onboarding/Rubric";
import type { PickRole } from "@/components/onboarding/data/models";
import { formatModelSize } from "@/lib/utils/format";
import { languageName } from "@/lib/constants/languages";
import { useModelStore } from "@/stores/modelStore";
import { useSettingsStore } from "@/stores/settingsStore";
import {
  cancelDownload,
  pickModel,
  useIsTarget,
  usePlace,
  type Entry,
  type Facts,
} from "./modelEntries";
import {
  ROW_GRID,
  columnsClass,
  columnsStyle,
  gradeMode,
  isDense,
} from "./columns";

const ROLE_KEYS: Record<PickRole, string> = {
  pick: "recommended",
  faster: "faster",
  accurate: "accurate",
};

/**
 * One model, onboarding's Show all row. The mark at its start is where the
 * model is: a radio once it's on disk (filled when it's the one in use), an
 * arrow while it's only in the catalog, a ring while it downloads. Pressing
 * the row does the one thing the mark promises: use it, or download it and
 * then use it. The slot at the end deletes it (asking first, in the row) or
 * stops its download.
 */
export const ModelRow: React.FC<{ entry: Entry; facts: Facts }> = ({
  entry,
  facts,
}) => {
  const { t, i18n } = useTranslation();
  const rubric = useRubric();
  const place = usePlace(entry.id);
  const target = useIsTarget(entry.id);
  const percent = useModelStore(
    (s) => s.downloadProgress[entry.id]?.percentage ?? 0,
  );
  const debug = useSettingsStore((s) => s.settings?.debug_mode ?? false);
  const [asking, setAsking] = useState(false);
  const askRef = useRef<HTMLSpanElement>(null);

  useEffect(() => {
    if (asking) askRef.current?.querySelector("button")?.focus();
  }, [asking]);

  const ui = i18n.language;
  const nameOf = (l: string) => languageName(l, ui);
  const list = (codes: string[]) =>
    new Intl.ListFormat(ui, { type: "conjunction" }).format(codes.map(nameOf));
  const { langs } = facts;
  const role = facts.roleOf(entry);
  const missing = facts.missing(entry);
  const dim = missing.length > 0;
  const onDisk = place === "active" || place === "ready" || place === "loading";
  const on = place === "active";
  const size = formatModelSize(entry.sizeMb);
  const wait = facts.wait(entry);
  const graded = !!entry.model;

  /* ---------------- the tooltips, as onboarding's ---------------- */

  const gradeTip = (ls: string[], chip: React.ReactElement) => {
    const one = ls.length === 1;
    const grade = one ? facts.grade(entry, ls[0]) : facts.overall(entry);
    const key = (k: string) => `onboarding.model.rubric.${k}`;
    const miss = ls.filter((l) => facts.grade(entry, l).unsupported);
    let text: React.ReactNode;
    if (!graded) text = t(key("unrated"), { language: list(ls) });
    else if (miss.length)
      text = t(key("unsupported"), { language: list(miss) });
    else if (grade.letter === "none")
      text = t(key("unrated"), { language: list(ls) });
    else text = t(key(`meaning.${grade.letter}`));
    // Several languages: each one's grade under the line, as words (the
    // chips' greys are for the page, not the tooltip's plum).
    const breakdown = !one && (
      <span className="mt-[6px] grid grid-cols-[1fr_auto] gap-x-[16px] tabular-nums">
        {ls.map((l) => (
          <React.Fragment key={l}>
            <span>{nameOf(l)}</span>
            <b className="text-end font-semibold">
              {facts.grade(entry, l).label}
            </b>
          </React.Fragment>
        ))}
      </span>
    );
    return (
      <Tooltip
        key={ls.join()}
        title={
          one
            ? t(key(grade.letter !== "none" ? "gradeTitle" : "accuracyTitle"), {
                language: nameOf(ls[0]),
                grade: grade.label,
              })
            : t(key(grade.letter !== "none" ? "overallTitle" : "overall"), {
                grade: grade.label,
              })
        }
        text={
          <>
            {text}
            {breakdown}
          </>
        }
      >
        {chip}
      </Tooltip>
    );
  };

  const speedWords =
    wait == null
      ? t("onboarding.model.speedUntested")
      : t(
          `onboarding.model.${entry.streaming ? "speedWordsPreview" : "speedWords"}.${speedLevel(wait)}`,
        );
  const speedTip = (bars: React.ReactElement) => (
    <Tooltip
      title={speedWords}
      text={
        wait == null
          ? t("onboarding.model.rubric.untimed")
          : t(`onboarding.model.rubric.speedText.${facts.tier}`, {
              band: rubric.speedBand(speedLevel(wait)),
            })
      }
    >
      {bars}
    </Tooltip>
  );

  /* ---------------- the pieces ---------------- */

  const mark = (() => {
    switch (place) {
      case "active":
        return <Radio checked />;
      case "ready":
        return <Radio checked={false} />;
      case "loading":
        return (
          <Loader2
            className="size-[18px] animate-spin text-ink-3"
            aria-hidden="true"
          />
        );
      case "downloading":
      case "checking":
        return (
          <span
            className={place === "checking" ? "grid animate-pulse" : "grid"}
          >
            <ProgressRing
              size={18}
              value={place === "checking" ? 1 : percent / 100}
            />
          </span>
        );
      case "remote":
        return (
          <span
            aria-hidden="true"
            className="grid size-[18px] place-items-center rounded-full border-[1.5px] border-dashed border-line-strong text-ink-3 transition-colors group-hover:border-solid group-hover:border-text group-hover:text-text"
          >
            <ArrowDown className="size-[11px]" strokeWidth={2.75} />
          </span>
        );
    }
  })();

  const capability = (key: "preview" | "translate", Icon: typeof LiveIcon) => (
    <Tooltip
      title={t(`onboarding.model.tags.${key}`)}
      text={t(`onboarding.model.tags.${key}Title`)}
    >
      <span className="grid size-[18px] place-items-center hover:text-text">
        <Icon
          role="img"
          aria-label={t(`onboarding.model.tags.${key}`)}
          className="size-[13px]"
        />
      </span>
    </Tooltip>
  );
  const capabilities = (entry.streaming || entry.translate) && (
    <span className="inline-flex shrink-0 items-center text-ink-3">
      {entry.streaming && capability("preview", LiveIcon)}
      {entry.translate && capability("translate", Languages)}
    </span>
  );

  const line = (() => {
    if (asking)
      return (
        <span className="text-text">
          {t(
            on
              ? "settings.models.deleteAskActive"
              : "settings.models.deleteAsk",
            { name: entry.name, size },
          )}
        </span>
      );
    if (place === "loading") return t("settings.models.loading");
    if (place === "checking") return t("onboarding.download.checking");
    if (place === "downloading")
      return (
        <span className="tabular-nums">
          {t(
            target
              ? "settings.models.downloadingThenUse"
              : "settings.models.downloadingLine",
            { percent: Math.round(percent), total: size },
          )}
        </span>
      );
    if (dim)
      return t("settings.models.doesntHear", { languages: list(missing) });
    if (entry.kind === "custom") return t("settings.models.customLine");
    if (entry.kind === "legacy") return t("settings.models.legacyLine");
    return entry.description;
  })();

  const columns = (
    <span
      style={columnsStyle(langs)}
      className={`grid items-center justify-items-center ${columnsClass(langs)}`}
    >
      {gradeMode(langs) === "each"
        ? langs.map((l) => {
            const g = facts.grade(entry, l);
            return gradeTip(
              [l],
              <span className="inline-grid">
                <GradeChip
                  letter={g.letter}
                  label={g.label}
                  dense={isDense(langs)}
                />
              </span>,
            );
          })
        : (() => {
            const g = facts.overall(entry);
            return gradeTip(
              langs,
              <span className="inline-grid">
                <GradeChip letter={g.letter} label={g.label} />
              </span>,
            );
          })()}
      {speedTip(
        <span className="inline-grid h-[22px] w-[36px] place-items-center">
          <SpeedBars level={speedLevel(wait)} className="h-[12px]!" />
        </span>,
      )}
      <span className="whitespace-nowrap text-label tabular-nums text-ink-3">
        {entry.sizeMb > 0 ? size : "—"}
      </span>
    </span>
  );

  // Pressing a download makes it the one to switch to when it's done.
  const act = () => {
    if (asking || on || place === "loading") return;
    pickModel(entry.id);
  };

  const slotClass =
    "grid size-[24px] cursor-pointer place-items-center rounded-[7px] text-ink-3 hover:bg-line hover:text-text focus:outline-none focus-visible:ring-1 focus-visible:ring-logo-primary";
  const slot = (() => {
    if (place === "downloading" || place === "checking") {
      const label = t("settings.models.cancelDownload", { name: entry.name });
      return (
        <button
          type="button"
          onClick={(e) => {
            e.stopPropagation();
            cancelDownload(entry.id);
          }}
          title={label}
          aria-label={label}
          className={slotClass}
        >
          <X className="size-[14px]" aria-hidden="true" />
        </button>
      );
    }
    if (onDisk) {
      const label = t("settings.models.deleteModel", { name: entry.name });
      return (
        <button
          type="button"
          onClick={(e) => {
            e.stopPropagation();
            setAsking(true);
          }}
          title={label}
          aria-label={label}
          className={`${slotClass} opacity-40 transition-opacity group-hover:opacity-100 focus-visible:opacity-100`}
        >
          <Trash2 className="size-[14px]" aria-hidden="true" />
        </button>
      );
    }
    return <span />;
  })();

  const ask = (
    <span
      ref={askRef}
      className="col-span-2 flex items-center justify-end gap-[6px]"
    >
      <Button
        variant="secondary"
        size="sm"
        onClick={(e) => {
          e.stopPropagation();
          setAsking(false);
        }}
      >
        {t("settings.models.cancel")}
      </Button>
      <Button
        variant="danger"
        size="sm"
        onClick={(e) => {
          e.stopPropagation();
          setAsking(false);
          void useModelStore.getState().deleteModel(entry.id);
        }}
      >
        {t("common.delete")}
      </Button>
    </span>
  );

  return (
    <div
      role="radio"
      aria-checked={on}
      tabIndex={0}
      onClick={act}
      onKeyDown={(e) => {
        // Escape closes the delete question from its buttons too.
        if (e.key === "Escape" && asking) {
          e.stopPropagation();
          setAsking(false);
          (e.currentTarget as HTMLElement).focus();
          return;
        }
        if (e.target !== e.currentTarget) return;
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          act();
        }
        if ((e.key === "Delete" || e.key === "Backspace") && onDisk)
          setAsking(true);
      }}
      title={place === "remote" ? t("settings.models.download") : undefined}
      className={`group relative ${ROW_GRID} min-h-[50px] w-full min-w-0 flex-none rounded-control px-[14px] py-[9px] text-start text-small leading-[1.5] text-text focus:outline-none focus-visible:ring-1 focus-visible:ring-logo-primary ${
        asking
          ? "bg-wash"
          : on
            ? // A quiet grey fill, a step past hover's wash.
              "cursor-default bg-line"
            : "cursor-pointer hover:bg-wash"
      }`}
    >
      <span className="grid place-items-center">{mark}</span>
      <span className="flex min-w-0 flex-col items-start">
        <span className="flex min-w-0 max-w-full items-center gap-[6px]">
          <b
            title={entry.name}
            className={`min-w-0 overflow-hidden text-ellipsis whitespace-nowrap font-[650] ${dim ? "opacity-60" : ""}`}
          >
            {entry.name}
          </b>
          {role && !asking && (
            <Tag>{t(`onboarding.model.roles.${ROLE_KEYS[role]}`)}</Tag>
          )}
          {entry.kind === "custom" && <Tag>{t("modelSelector.custom")}</Tag>}
          {entry.kind === "legacy" && (
            <Tag>{t("onboarding.migration.oldFormat")}</Tag>
          )}
          {capabilities}
          {debug && entry.quant && (
            <span className="shrink-0 text-tag font-semibold text-ink-3">
              {entry.quant}
            </span>
          )}
        </span>
        <span
          title={typeof line === "string" ? line : undefined}
          className={`max-w-full overflow-hidden text-ellipsis whitespace-nowrap text-ink-3 ${dim && !asking ? "opacity-60" : ""}`}
        >
          {line}
        </span>
      </span>
      {asking ? ask : columns}
      {!asking && <span className="grid place-items-center">{slot}</span>}
    </div>
  );
};

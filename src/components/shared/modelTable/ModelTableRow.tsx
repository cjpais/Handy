import React from "react";
import { useTranslation } from "react-i18next";
import { Languages, Radio as LiveIcon } from "lucide-react";
import { Tooltip } from "@/components/ui/Tooltip";
import { ProgressRing } from "@/components/shared/ProgressRing";
import { Tag } from "@/components/shared/Tag";
import { formatModelSize } from "@/lib/utils/format";
import { useSettingsStore } from "@/stores/settingsStore";
import type { PickRole } from "@/components/onboarding/data/models";
import { Values, rowGrid, type VtName } from "./columns";
import { useModelTable, type TableModel } from "./context";

export const ROLE_KEYS: Record<PickRole, string> = {
  pick: "recommended",
  faster: "faster",
  accurate: "accurate",
};

export interface RowDownload {
  state: "downloading" | "checking";
  /** 0–100. */
  percent: number;
}

interface ModelTableRowProps {
  entry: TableModel;
  /** The page's picked one: in use (Models page), to download (onboarding). */
  selected: boolean;
  /** The page's mark in the first column, while nothing downloads. */
  mark: React.ReactNode;
  /** Downloading: a ring in the mark's place, how far on the second line. */
  download?: RowDownload | null;
  /** The page's own second line, over the download's and the description. */
  line?: React.ReactNode;
  /** The delete/stop column's button (the Models page's). */
  slot?: React.ReactNode;
  /** Takes the columns' and the slot's place (the delete question). */
  tail?: React.ReactNode;
  onActivate: () => void;
  /** Runs first; preventDefault() keeps Enter and Space from activating. */
  onKeyDown?: (e: React.KeyboardEvent<HTMLDivElement>) => void;
  hint?: string;
  /** Onboarding's moves: the row's parts carry view-transition names. */
  vt?: VtName;
}

/**
 * One model in the table, the same on onboarding and the Models page: the
 * page's mark (a ring while it downloads), the name with its role, kind and
 * what it can do besides transcribe, its description (or how far the
 * download is), then the columns and, on the Models page, delete or stop.
 */
export const ModelTableRow: React.FC<ModelTableRowProps> = ({
  entry,
  selected,
  mark,
  download,
  line,
  slot,
  tail,
  onActivate,
  onKeyDown,
  hint,
  vt,
}) => {
  const { t } = useTranslation();
  const { grading, actions, highlighted } = useModelTable();
  const debug = useSettingsStore((s) => s.settings?.debug_mode ?? false);
  const role = grading.roleOf(entry.model);
  const dim = grading.missing(entry.model).length > 0 && !tail;

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

  const shownMark = download ? (
    <span
      className={download.state === "checking" ? "grid animate-pulse" : "grid"}
    >
      <ProgressRing
        size={18}
        value={download.state === "checking" ? 1 : download.percent / 100}
      />
    </span>
  ) : (
    mark
  );

  const shownLine =
    line ??
    (download ? (
      download.state === "checking" ? (
        t("onboarding.download.checking")
      ) : (
        <span className="tabular-nums">
          {t("settings.models.downloadingLine", {
            percent: Math.round(download.percent),
            total: formatModelSize(entry.sizeMb),
          })}
        </span>
      )
    ) : entry.kind === "custom" ? (
      t("settings.models.customLine")
    ) : entry.kind === "legacy" ? (
      t("settings.models.legacyLine")
    ) : (
      entry.description
    ));

  return (
    <div
      role="radio"
      aria-checked={selected}
      tabIndex={0}
      data-row={entry.id}
      {...vt?.()}
      data-vt-row={vt ? "" : undefined}
      onClick={onActivate}
      onKeyDown={(e) => {
        onKeyDown?.(e);
        if (e.defaultPrevented || e.target !== e.currentTarget) return;
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          onActivate();
        }
      }}
      title={hint}
      className={`group relative ${rowGrid(actions)} min-h-[50px] w-full min-w-0 flex-none rounded-control px-[14px] py-[9px] text-start text-small leading-[1.5] text-text focus:outline-none focus-visible:ring-1 focus-visible:ring-logo-primary ${vt ? "[view-transition-class:mf-row_mf-box]" : ""} ${
        tail
          ? "bg-wash"
          : selected
            ? // A quiet grey fill, a step past hover's wash.
              "cursor-default bg-line"
            : `cursor-pointer hover:bg-wash ${highlighted === entry.id ? "bg-wash" : ""}`
      }`}
    >
      <span {...vt?.("m")} className="grid place-items-center">
        {shownMark}
      </span>
      <span className="flex min-w-0 flex-col items-start">
        <span className="flex min-w-0 max-w-full items-center gap-[6px]">
          <b
            {...vt?.("n")}
            title={entry.name}
            className={`min-w-0 overflow-hidden text-ellipsis whitespace-nowrap font-[650] ${dim ? "opacity-60" : ""}`}
          >
            {entry.name}
          </b>
          {/* The delete question needs the room. */}
          {role && !tail && (
            <Tag {...vt?.("r")}>
              {t(`onboarding.model.roles.${ROLE_KEYS[role]}`)}
            </Tag>
          )}
          {entry.kind === "custom" && <Tag>{t("modelSelector.custom")}</Tag>}
          {entry.kind === "legacy" && (
            <Tag>{t("onboarding.migration.oldFormat")}</Tag>
          )}
          {(entry.streaming || entry.translate) && (
            <span className="inline-flex shrink-0 items-center text-ink-3">
              {entry.streaming && capability("preview", LiveIcon)}
              {entry.translate && capability("translate", Languages)}
            </span>
          )}
          {debug && entry.quant && (
            <span className="shrink-0 text-tag font-semibold text-ink-3">
              {entry.quant}
            </span>
          )}
        </span>
        <span
          {...vt?.("d")}
          title={typeof shownLine === "string" ? shownLine : undefined}
          className={`max-w-full overflow-hidden text-ellipsis whitespace-nowrap text-ink-3 ${dim ? "opacity-60" : ""}`}
        >
          {shownLine}
        </span>
      </span>
      {tail ?? (
        <Values
          grading={grading}
          model={entry.model}
          streaming={entry.streaming}
          sizeMb={entry.sizeMb}
          vt={vt}
        />
      )}
      {actions && !tail && (
        <span className="grid place-items-center">{slot}</span>
      )}
    </div>
  );
};

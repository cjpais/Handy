import React from "react";
import { useTranslation } from "react-i18next";
import { Tooltip } from "@/components/ui/Tooltip";
import { ProgressRing } from "@/components/shared/ProgressRing";
import { Tag } from "@/components/shared/Tag";
import { formatModelSize } from "@/lib/utils/format";
import { useSettingsStore } from "@/stores/settingsStore";
import type { PickRole } from "@/lib/models/catalog";
import { CAPABILITIES, CAPABILITY_KEYS, type Capability } from "./capabilities";
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
  /** In use (Models page) or picked to download (onboarding). */
  selected: boolean;
  /** First-column mark; replaced by a ring while downloading. */
  mark: React.ReactNode;
  /** Download progress, shown as a ring and on the second line. */
  download?: RowDownload | null;
  /** Second line; takes precedence over download progress and description. */
  line?: React.ReactNode;
  /** Delete/stop button (Models page). */
  slot?: React.ReactNode;
  /** Replaces the columns and slot (the delete confirmation). */
  tail?: React.ReactNode;
  onActivate: () => void;
  /** Runs first; preventDefault() keeps Enter and Space from activating. */
  onKeyDown?: (e: React.KeyboardEvent<HTMLDivElement>) => void;
  hint?: string;
  /** Can't be picked right now: no hover, a faded mark. Still clickable. */
  inert?: boolean;
  /** Downloaded: never faded for missing your languages. */
  onDisk?: boolean;
  /** View-transition names for onboarding's animations. */
  vt?: VtName;
}

/** One model row, shared by onboarding and the Models page. */
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
  inert = false,
  onDisk = false,
  vt,
}) => {
  const { t } = useTranslation();
  const { grading, actions, highlighted } = useModelTable();
  const debug = useSettingsStore((s) => s.settings?.debug_mode ?? false);
  const role = grading.roleOf(entry.model);
  const dim = grading.missing(entry.model).length > 0 && !tail && !onDisk;
  const capabilities = CAPABILITY_KEYS.filter(
    (c) => entry[CAPABILITIES[c].flag],
  );

  const capability = (key: Capability) => {
    const { Icon } = CAPABILITIES[key];
    return (
      <Tooltip
        key={key}
        title={t(`modelTable.tags.${key}`)}
        text={t(`modelTable.tags.${key}Title`)}
      >
        <span className="grid size-[18px] place-items-center hover:text-text">
          <Icon
            role="img"
            aria-label={t(`modelTable.tags.${key}`)}
            className="size-[13px]"
          />
        </span>
      </Tooltip>
    );
  };

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
          {t("modelTable.downloadingLine", {
            percent: Math.round(download.percent),
            total: formatModelSize(entry.sizeMb),
          })}
        </span>
      )
    ) : entry.kind === "custom" ? (
      t("modelTable.customLine")
    ) : entry.kind === "legacy" ? (
      t("modelTable.legacyLine")
    ) : (
      entry.description
    ));

  return (
    <div
      role="radio"
      aria-checked={selected}
      aria-disabled={inert || undefined}
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
            : `${inert ? "cursor-default" : "cursor-pointer hover:bg-wash"} ${highlighted === entry.id ? "bg-wash" : ""}`
      }`}
    >
      <span
        {...vt?.("m")}
        className={`grid place-items-center ${inert ? "opacity-40" : ""}`}
      >
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
            <Tag {...vt?.("r")}>{t(`modelTable.roles.${ROLE_KEYS[role]}`)}</Tag>
          )}
          {entry.kind === "custom" && <Tag>{t("modelSelector.custom")}</Tag>}
          {entry.kind === "legacy" && (
            <Tag>{t("onboarding.migration.oldFormat")}</Tag>
          )}
          {capabilities.length > 0 && (
            <span className="inline-flex shrink-0 items-center text-ink-3">
              {capabilities.map(capability)}
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

import React, { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { ArrowDown, Loader2, Trash2, X } from "lucide-react";
import { Radio } from "@/components/ui/Radio";
import { Button } from "@/components/ui/Button";
import { ModelTableRow } from "@/components/shared/modelTable/ModelTableRow";
import { formatModelSize } from "@/lib/utils/format";
import { useModelStore } from "@/stores/modelStore";
import {
  cancelDownload,
  pickModel,
  useIsTarget,
  usePlace,
  type Entry,
} from "./modelEntries";

/** A Models page row: the shared table row plus use, stop and delete. */
export const ModelRow: React.FC<{ entry: Entry }> = ({ entry }) => {
  const { t } = useTranslation();
  const place = usePlace(entry.id);
  const target = useIsTarget(entry.id);
  const percent = useModelStore(
    (s) => s.downloadProgress[entry.id]?.percentage ?? 0,
  );
  const [asking, setAsking] = useState(false);
  const askRef = useRef<HTMLSpanElement>(null);

  useEffect(() => {
    if (asking) askRef.current?.querySelector("button")?.focus();
  }, [asking]);

  const onDisk = place === "active" || place === "ready" || place === "loading";
  const on = place === "active";
  const size = formatModelSize(entry.sizeMb);
  const download =
    place === "downloading" || place === "checking"
      ? { state: place, percent }
      : null;

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
      case "remote":
        return (
          <span
            aria-hidden="true"
            className="grid size-[18px] place-items-center rounded-full border-[1.5px] border-dashed border-line-strong text-ink-3 transition-colors group-hover:border-solid group-hover:border-text group-hover:text-text"
          >
            <ArrowDown className="size-[11px]" strokeWidth={2.75} />
          </span>
        );
      default:
        return null;
    }
  })();

  // Page-only states; the table handles downloads, custom and old models.
  const line = asking ? (
    <span className="text-text">
      {t(on ? "settings.models.deleteAskActive" : "settings.models.deleteAsk", {
        name: entry.name,
        size,
      })}
    </span>
  ) : place === "loading" ? (
    t("settings.models.loading")
  ) : place === "downloading" && target ? (
    <span className="tabular-nums">
      {t("settings.models.downloadingThenUse", {
        percent: Math.round(percent),
        total: size,
      })}
    </span>
  ) : undefined;

  // Pressing a download makes it the one to switch to when it's done.
  const act = () => {
    if (asking || on || place === "loading") return;
    pickModel(entry.id);
  };

  const slotClass =
    "grid size-[24px] cursor-pointer place-items-center rounded-[7px] text-ink-3 hover:bg-line hover:text-text focus:outline-none focus-visible:ring-1 focus-visible:ring-logo-primary";
  const slot = (() => {
    if (download) {
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

  const ask = asking && (
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
    <ModelTableRow
      entry={entry}
      selected={on}
      mark={mark}
      download={download}
      line={line}
      slot={slot}
      tail={ask || undefined}
      onActivate={act}
      onKeyDown={(e) => {
        // Escape closes the delete question from its buttons too.
        if (e.key === "Escape" && asking) {
          e.preventDefault();
          e.stopPropagation();
          setAsking(false);
          (e.currentTarget as HTMLElement).focus();
          return;
        }
        if (e.target !== e.currentTarget) return;
        if ((e.key === "Delete" || e.key === "Backspace") && onDisk) {
          e.preventDefault();
          setAsking(true);
        }
      }}
      hint={place === "remote" ? t("settings.models.download") : undefined}
    />
  );
};

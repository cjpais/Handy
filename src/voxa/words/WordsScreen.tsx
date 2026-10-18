import React, { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { listen } from "@tauri-apps/api/event";
import { commands, events } from "@/bindings";
import type { BackfillStatus } from "@/bindings";
import { useWordDetail, useWordsData } from "./useWordsData";
import { isoDay, normalizeRange, periodFor, type PeriodPreset } from "./format";
import { WordsView, type SelectedWord } from "./WordsView";

const BACKFILL_EVENT = "voxa-words-backfill";

/** Period, filters and loading for the Words tab; the view is `WordsView`. */
export const WordsScreen: React.FC = () => {
  const { i18n } = useTranslation();
  const [preset, setPreset] = useState<PeriodPreset>("week");
  const [custom, setCustom] = useState(() => {
    const today = new Date();
    return {
      from: periodFor("month", today).from ?? isoDay(today),
      to: isoDay(today),
    };
  });
  const [appliedCustom, setAppliedCustom] = useState(custom);
  const [includeStop, setIncludeStop] = useState(false);
  const [selected, setSelected] = useState<SelectedWord | null>(null);
  const [backfill, setBackfill] = useState<BackfillStatus | null>(null);
  const [reloadKey, setReloadKey] = useState(0);
  const reload = useCallback(() => setReloadKey((k) => k + 1), []);

  // "Today" is fixed per mount; a reload keeps rolling windows honest.
  const period = useMemo(
    () => periodFor(preset, new Date(), appliedCustom),
    // reloadKey re-reads today
    [preset, appliedCustom, reloadKey],
  );

  const data = useWordsData(period, includeStop, reloadKey);
  const detail = useWordDetail(period, selected?.stem ?? null, reloadKey);

  // progress of the one-time indexing of older dictations
  useEffect(() => {
    let cancelled = false;
    void commands.voxaWordsBackfillStatus().then((r) => {
      if (!cancelled && r.status === "ok") setBackfill(r.data);
    });
    const unlisten = listen<BackfillStatus>(BACKFILL_EVENT, (e) => {
      setBackfill(e.payload);
      if (e.payload.state === "done") reload();
    });
    return () => {
      cancelled = true;
      void unlisten.then((fn) => fn());
    };
  }, [reload]);

  // a new dictation lands in the aggregates just after its history entry
  useEffect(() => {
    let timer: ReturnType<typeof setTimeout> | undefined;
    const unlisten = events.historyUpdatePayload.listen((event) => {
      if (event.payload.action !== "added") return;
      clearTimeout(timer);
      timer = setTimeout(reload, 900);
    });
    return () => {
      clearTimeout(timer);
      void unlisten.then((fn) => fn());
    };
  }, [reload]);

  const onCustom = (from: string, to: string) => {
    setCustom({ from, to });
    const range = normalizeRange(from, to);
    if (range) setAppliedCustom(range);
  };

  return (
    <WordsView
      data={data}
      locale={i18n.language}
      preset={preset}
      custom={custom}
      onPreset={setPreset}
      onCustom={onCustom}
      includeStop={includeStop}
      onIncludeStop={setIncludeStop}
      backfill={backfill}
      selected={selected}
      detail={detail}
      onSelect={setSelected}
      onRetry={reload}
    />
  );
};

export default WordsScreen;

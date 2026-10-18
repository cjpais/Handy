import { useCallback, useEffect, useRef, useState } from "react";
import { commands } from "@/bindings";
import type { Result, WordsPeriod } from "@/bindings";
import { emptyWordsData, type Block, type WordsData } from "./blocks";

type Loader<T> = () => Promise<Result<T, string>>;

/**
 * Loads the blocks of the Words tab, each on its own, so a slow or failing one
 * does not hold up the others. A newer request for the same block wins; an
 * older answer arriving late is dropped.
 */
export function useWordsData(
  period: WordsPeriod,
  includeStop: boolean,
  reloadKey: number,
): WordsData {
  const [data, setData] = useState<WordsData>(emptyWordsData);
  const generation = useRef(0);

  const periodKey = `${period.from ?? ""}|${period.to ?? ""}`;

  useEffect(() => {
    const mine = ++generation.current;
    const run = <K extends keyof WordsData>(
      key: K,
      loader: Loader<WordsData[K] extends Block<infer T> ? T : never>,
    ) => {
      loader()
        .then((result) => {
          if (generation.current !== mine) return;
          setData((prev) => ({
            ...prev,
            [key]:
              result.status === "ok"
                ? { status: "ready", data: result.data }
                : { status: "error", error: result.error },
          }));
        })
        .catch((e) => {
          if (generation.current !== mine) return;
          setData((prev) => ({
            ...prev,
            [key]: { status: "error", error: String(e) },
          }));
        });
    };
    run("top", () => commands.voxaWordsTop(period, includeStop));
    run("fresh", () => commands.voxaWordsNew(period, includeStop));
    run("vocabulary", () => commands.voxaWordsVocabulary(period));
    run("openers", () => commands.voxaWordsOpeners(period));
    run("phrases", () => commands.voxaWordsPhrases(period));
    run("fillers", () => commands.voxaWordsFillers(period));
    // `periodKey` stands for `period` in the dependencies
  }, [periodKey, includeStop, reloadKey]);

  return data;
}

/** Details of one word in the period; `null` while nothing is selected. */
export function useWordDetail(
  period: WordsPeriod,
  stem: string | null,
  reloadKey: number,
): Block<import("@/bindings").WordDetail> | null {
  const [detail, setDetail] = useState<Block<
    import("@/bindings").WordDetail
  > | null>(null);
  const generation = useRef(0);
  const periodKey = `${period.from ?? ""}|${period.to ?? ""}`;

  const load = useCallback(() => {
    if (!stem) {
      setDetail(null);
      return;
    }
    const mine = ++generation.current;
    setDetail({ status: "loading" });
    commands
      .voxaWordDetail(period, stem)
      .then((result) => {
        if (generation.current !== mine) return;
        setDetail(
          result.status === "ok"
            ? { status: "ready", data: result.data }
            : { status: "error", error: result.error },
        );
      })
      .catch((e) => {
        if (generation.current === mine)
          setDetail({ status: "error", error: String(e) });
      });
    // `periodKey` stands for `period` in the dependencies
  }, [stem, periodKey]);

  useEffect(() => {
    load();
  }, [load, reloadKey]);

  return detail;
}

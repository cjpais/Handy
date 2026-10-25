/**
 * What the Models page knows about each model: the app's own list (what's on
 * disk, downloading, in use) joined to the catalog by repository, so the
 * grades, speeds and picks are onboarding's own for your languages.
 */
import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import { create } from "zustand";
import { catalog } from "@handy-computer/models";
import type { ModelInfo } from "@/bindings";
import { useModelStore } from "@/stores/modelStore";
import { useSettings } from "@/hooks/useSettings";
import {
  CURATED_ASR,
  groupModels,
  picksFor,
} from "@/components/onboarding/data/models";
import {
  averageError,
  makeGrading,
  type Grading,
} from "@/components/shared/modelTable/grading";
import type { TableModel } from "@/components/shared/modelTable/context";
import { useMachineTier } from "@/components/onboarding/useOnboardingModels";
import { repoIdOf } from "@/components/onboarding/retiredModels";
import {
  getTranslatedModelDescription,
  getTranslatedModelName,
} from "@/lib/utils/modelTranslation";

// Legacy models are the blob (Url-sourced) .bin downloads, superseded by
// the catalog GGUFs. They stay runnable when already on disk, but we no longer
// advertise the download.
export const isLegacyModel = (model: ModelInfo): boolean =>
  typeof model.source === "object" && "Url" in model.source;

// Extract a GGUF quantization label from a filename, if present (e.g. "Q8_0").
const quantOf = (filename: string): string | null => {
  const match = filename.match(
    /[._-](IQ\d+_\w+|Q\d+(?:_\w+)?|F16|BF16|F32)\.gguf$/i,
  );
  return match ? match[1].toUpperCase() : null;
};

/** A model on the page: the table's own shape. */
export type Entry = TableModel;

const CATALOG_BY_REPO = new Map(
  catalog.models
    .filter((m) => m.role === "asr")
    .map((m) => [m.repository, m] as const),
);

/** The table's grading, and where each model ranks for your languages. */
export type Facts = Grading & {
  /** Lower is a better fit; the uncurated after every curated model. */
  rankOf: (e: Entry) => number;
};

export interface Lists {
  /** On disk: the one in use first, then best fit; custom and old last. */
  yours: Entry[];
  /** Curated, not on disk, hearing all your languages: the picks first. */
  fits: Entry[];
  /** Curated, not on disk, missing one of your languages. */
  partial: Entry[];
  /** Not curated, not on disk: only found by searching. */
  untested: Entry[];
}

const KIND_ORDER = { curated: 0, catalog: 0, custom: 1, legacy: 2 } as const;

/**
 * Every model the page shows, graded for your languages (`spoken_languages`)
 * on this computer's tier, and sorted into the page's sections.
 */
export const useModelEntries = () => {
  const { t } = useTranslation();
  const { getSetting } = useSettings();
  const spoken = getSetting("spoken_languages");
  const langs = useMemo(
    () => (spoken?.length ? spoken : ["en"]),
    [spoken?.join()],
  );
  const tier = useMachineTier();
  const infos = useModelStore((s) => s.models);
  const current = useModelStore((s) => s.currentModel);

  const entries = useMemo(
    () =>
      infos
        // Old .bin downloads only while they're still on disk.
        .filter((info) => !isLegacyModel(info) || info.is_downloaded)
        .map((info): Entry => {
          const repo = repoIdOf(info);
          const model =
            info.is_custom || !repo ? undefined : CATALOG_BY_REPO.get(repo);
          return {
            id: info.id,
            name: getTranslatedModelName(info, t),
            description: getTranslatedModelDescription(info, t),
            languages: info.supported_languages,
            streaming: info.supports_streaming,
            translate: info.supports_translation,
            sizeMb: Number(info.size_mb),
            // The file's own, so a different quant found on disk says so.
            quant: quantOf(info.filename) ?? model?.quant ?? null,
            kind: info.is_custom
              ? "custom"
              : isLegacyModel(info)
                ? "legacy"
                : model?.curated
                  ? "curated"
                  : "catalog",
            model,
          };
        }),
    [infos, t],
  );

  const facts = useMemo((): Facts => {
    const offered = new Set(
      entries.flatMap((e) => (e.kind === "curated" ? [e.model!.id] : [])),
    );
    const curated = CURATED_ASR.filter((m) => offered.has(m.id));
    const picks = picksFor(langs, (m) => offered.has(m.id));
    const groups = groupModels(langs, tier, curated, picks);
    const rank = new Map(
      [...groups.picks, ...groups.others, ...groups.partial].map((r, i) => [
        r.model.id,
        i,
      ]),
    );
    const grading = makeGrading(langs, tier, picks);
    return {
      ...grading,
      rankOf: (e) => {
        const r = e.model && rank.get(e.model.id);
        if (r != null) return r;
        if (!e.model) return 10_000;
        return (
          1000 +
          grading.missing(e.model).length * 100 +
          (averageError(e.model, langs) ?? 99)
        );
      },
    };
  }, [entries, langs, tier]);

  const lists = useMemo((): Lists => {
    const infoById = new Map(infos.map((i) => [i.id, i]));
    // A download stays where it was pressed until it's done.
    const onDisk = (e: Entry) => !!infoById.get(e.id)?.is_downloaded;
    const byFit = (a: Entry, b: Entry) => facts.rankOf(a) - facts.rankOf(b);
    const yours = entries
      .filter(onDisk)
      .sort(
        (a, b) =>
          Number(b.id === current) - Number(a.id === current) ||
          KIND_ORDER[a.kind] - KIND_ORDER[b.kind] ||
          byFit(a, b),
      );
    const rest = entries.filter((e) => !onDisk(e));
    const curated = rest.filter((e) => e.kind === "curated").sort(byFit);
    return {
      yours,
      fits: curated.filter((e) => !facts.missing(e.model).length),
      partial: curated.filter((e) => facts.missing(e.model).length > 0),
      untested: rest.filter((e) => e.kind !== "curated").sort(byFit),
    };
  }, [entries, infos, current, facts]);

  return { facts, lists };
};

/* ---------------- where a model is, and what pressing it does ---------------- */

interface PageState {
  /** Loading into memory after it was picked. */
  switching: string | null;
  /** A download that becomes the model in use when it's done. */
  target: string | null;
}

const usePageState = create<PageState>(() => ({
  switching: null,
  target: null,
}));

/** Where a model is: in use, on disk, on its way, or only in the catalog. */
export type Place =
  | "active"
  | "loading"
  | "ready"
  | "downloading"
  | "checking"
  | "remote";

export const usePlace = (id: string): Place => {
  const switching = usePageState((s) => s.switching === id);
  const verifying = useModelStore((s) => id in s.verifyingModels);
  const downloading = useModelStore((s) => id in s.downloadingModels);
  const current = useModelStore((s) => s.currentModel === id);
  const downloaded = useModelStore(
    (s) => !!s.models.find((m) => m.id === id)?.is_downloaded,
  );
  if (switching) return "loading";
  if (verifying) return "checking";
  if (downloading) return "downloading";
  // A stale selection must never make a missing model look in use.
  if (!downloaded) return "remote";
  return current ? "active" : "ready";
};

export const useIsTarget = (id: string) => usePageState((s) => s.target === id);

const select = async (id: string) => {
  usePageState.setState({ switching: id, target: null });
  try {
    await useModelStore.getState().selectModel(id);
  } finally {
    if (usePageState.getState().switching === id)
      usePageState.setState({ switching: null });
  }
};

/**
 * Use a model: at once when it's on disk, else download it and then use it.
 * The last one pressed wins; the others carry on downloading.
 */
export const pickModel = (id: string) => {
  const store = useModelStore.getState();
  if (store.models.find((m) => m.id === id)?.is_downloaded) {
    void select(id);
    return;
  }
  usePageState.setState({ target: id });
  if (id in store.downloadingModels) return;
  void store.downloadModel(id).then((ok) => {
    if (ok && usePageState.getState().target === id) void select(id);
  });
};

export const cancelDownload = (id: string) => {
  if (usePageState.getState().target === id)
    usePageState.setState({ target: null });
  void useModelStore.getState().cancelDownload(id);
};

import { useMemo } from "react";
import type { ModelInfo } from "@/bindings";
import { useModelStore } from "@/stores/modelStore";
import {
  CURATED_ASR,
  groupModels,
  picksFor,
  type CatalogModel,
} from "@/lib/models/catalog";
import { makeGrading } from "@/lib/models/grading";
import { repoIdOf } from "@/lib/models/retired";
import { useMachineTier } from "./useMachineTier";

/**
 * The app's models joined to the catalog by repo, with the picks, groups and
 * grades for these languages on this machine. Shared by onboarding and the
 * Models page; only curated models the app can download are offered.
 */
export const useModelCatalog = (languages: string[]) => {
  const tier = useMachineTier();
  const infos = useModelStore((s) => s.models);

  const infoByRepo = useMemo(() => {
    const map = new Map<string, ModelInfo>();
    for (const info of infos) {
      const repo = repoIdOf(info);
      if (repo && !info.is_custom && !map.has(repo)) map.set(repo, info);
    }
    return map;
  }, [infos]);

  const infoFor = (model: CatalogModel) => infoByRepo.get(model.repository);

  const offered = useMemo(
    () => CURATED_ASR.filter((m) => infoByRepo.has(m.repository)),
    [infoByRepo],
  );

  // Keyed by the codes: callers may pass a new array every render.
  const langsKey = languages.join();
  const langs = useMemo(
    () => (langsKey ? langsKey.split(",") : []),
    [langsKey],
  );

  const picks = useMemo(
    () => picksFor(langs, (m) => infoByRepo.has(m.repository)),
    [langs, infoByRepo],
  );

  const groups = useMemo(
    () => groupModels(langs, tier, offered, picks),
    [langs, tier, offered, picks],
  );

  const grading = useMemo(
    () => makeGrading(langs, tier, picks),
    [langs, tier, picks],
  );

  /** A catalog model for a ModelInfo id (the backend's), if it's offered. */
  const catalogFor = (infoId: string): CatalogModel | undefined =>
    offered.find((m) => infoByRepo.get(m.repository)?.id === infoId);

  return { tier, offered, picks, groups, grading, infoFor, catalogFor };
};

import { createContext, useContext } from "react";
import type { CatalogModel } from "@handy-computer/models";
import type { Grading } from "@/lib/models/grading";

/** Curated (onboarding offers it), catalog (untested), custom or legacy. */
export type ModelKind = "curated" | "catalog" | "custom" | "legacy";

/** A model as the table shows it, whichever page's list it came from. */
export interface TableModel {
  id: string;
  name: string;
  description: string;
  languages: string[];
  streaming: boolean;
  translate: boolean;
  sizeMb: number;
  quant: string | null;
  kind: ModelKind;
  /** Its catalog entry, which grades it; none for custom and legacy. */
  model?: CatalogModel;
}

interface TableContext {
  grading: Grading;
  /** The delete/stop column (the Models page's). */
  actions: boolean;
  /** The row highlighted by ↑↓ from the search. */
  highlighted: string | null;
}

export const ModelTableContext = createContext<TableContext | null>(null);

export const useModelTable = () => {
  const ctx = useContext(ModelTableContext);
  if (!ctx) throw new Error("A model row must be inside a ModelTable");
  return ctx;
};

import type {
  Fillers,
  Openers,
  Phrases,
  Vocabulary,
  WordCount,
  WordDetail,
} from "@/bindings";

/** One block of the Words tab loads, fails and retries on its own. */
export type Block<T> =
  | { status: "loading" }
  | { status: "ready"; data: T }
  | { status: "error"; error: string };

export interface WordsData {
  top: Block<WordCount[]>;
  fresh: Block<WordCount[]>;
  vocabulary: Block<Vocabulary>;
  openers: Block<Openers>;
  phrases: Block<Phrases>;
  fillers: Block<Fillers>;
}

export const LOADING: Block<never> = { status: "loading" };

export function emptyWordsData(): WordsData {
  return {
    top: LOADING,
    fresh: LOADING,
    vocabulary: LOADING,
    openers: LOADING,
    phrases: LOADING,
    fillers: LOADING,
  };
}

export type DetailBlock = Block<WordDetail>;

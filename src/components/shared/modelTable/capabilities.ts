import { Languages, Radio as LiveIcon } from "lucide-react";

/**
 * What a model can do besides transcribe: its icon, and its flag on a table
 * model and in the catalog's `capabilities`. Words under `modelTable.tags`.
 */
export const CAPABILITIES = {
  preview: { Icon: LiveIcon, flag: "streaming" },
  translate: { Icon: Languages, flag: "translate" },
} as const;

export type Capability = keyof typeof CAPABILITIES;

export const CAPABILITY_KEYS = Object.keys(CAPABILITIES) as Capability[];

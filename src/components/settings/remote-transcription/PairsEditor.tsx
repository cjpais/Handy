import React from "react";
import { useTranslation } from "react-i18next";
import { Plus, X } from "lucide-react";
import type { RequestPair } from "@/bindings";
import { BlurInput, K } from "./shared";

/** Name/value rows: the form fields or headers of a request. */
export const PairsEditor: React.FC<{
  pairs: RequestPair[];
  onChange: (pairs: RequestPair[]) => void;
  /** Which strings to use: `fields` or `headers` under this section. */
  kind: "fields" | "headers";
}> = ({ pairs, onChange, kind }) => {
  const { t } = useTranslation();
  const set = (i: number, patch: Partial<RequestPair>) =>
    onChange(pairs.map((p, j) => (j === i ? { ...p, ...patch } : p)));
  return (
    <div className="space-y-1.5">
      {pairs.map((pair, i) => (
        <div key={i} className="flex items-center gap-1.5">
          <BlurInput
            value={pair.name}
            placeholder={t(`${K}.${kind}.namePlaceholder`)}
            aria-label={t(`${K}.${kind}.nameLabel`)}
            className="!w-[38%] font-mono text-xs"
            onCommit={(name) => set(i, { name })}
          />
          <BlurInput
            value={pair.value}
            placeholder={t(`${K}.pairs.value`)}
            aria-label={t(`${K}.${kind}.valueLabel`)}
            className="flex-1 font-mono text-xs"
            onCommit={(value) => set(i, { value })}
          />
          <button
            type="button"
            onClick={() => onChange(pairs.filter((_, j) => j !== i))}
            aria-label={t(`${K}.pairs.remove`, { name: pair.name })}
            className="grid size-[26px] shrink-0 cursor-pointer place-items-center rounded-md text-ink-3 hover:bg-wash hover:text-text focus:outline-none focus-visible:ring-1 focus-visible:ring-logo-primary"
          >
            <X className="size-[13px]" aria-hidden="true" />
          </button>
        </div>
      ))}
      <button
        type="button"
        onClick={() => onChange([...pairs, { name: "", value: "" }])}
        className="flex cursor-pointer items-center gap-1 rounded-md px-1 py-0.5 text-xs font-medium text-ink-2 hover:text-text focus:outline-none focus-visible:ring-1 focus-visible:ring-logo-primary"
      >
        <Plus className="size-[12px]" aria-hidden="true" />
        {t(`${K}.${kind}.add`)}
      </button>
    </div>
  );
};

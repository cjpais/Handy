import React from "react";
import { useTranslation } from "react-i18next";
import type { RemoteServer } from "@/lib/remoteServers";
import { PairsEditor } from "./PairsEditor";
import { BlurInput, Field, K } from "./shared";

/** What the backend falls back to when a server has no timeout. */
const DEFAULT_TIMEOUT_SECS = 120;

/** The settings most servers never need changed, folded away. */
export const MoreOptions: React.FC<{
  draft: RemoteServer;
  commit: (patch: Partial<RemoteServer>) => void;
}> = ({ draft, commit }) => {
  const { t } = useTranslation();
  return (
    <details className="group px-4 py-1">
      <summary className="cursor-pointer select-none py-1 text-sm font-medium text-ink-2 hover:text-text">
        {t(`${K}.moreOptions`)}
      </summary>
      <div className="-mx-4 mt-1 pb-1">
        <Field
          title={t(`${K}.headers.label`)}
          description={t(`${K}.headers.description`)}
        >
          <PairsEditor
            pairs={draft.headers}
            kind="headers"
            onChange={(headers) => commit({ headers })}
          />
        </Field>
        {draft.body === "multipart" && (
          <Field
            title={t(`${K}.fileField.label`)}
            description={t(`${K}.fileField.description`)}
          >
            <BlurInput
              value={draft.file_field}
              aria-label={t(`${K}.fileField.label`)}
              placeholder="file"
              className="font-mono text-xs"
              onCommit={(file_field) => commit({ file_field })}
            />
          </Field>
        )}
        <Field
          title={t(`${K}.textPaths.label`)}
          description={t(`${K}.textPaths.description`)}
        >
          <BlurInput
            value={draft.text_paths}
            aria-label={t(`${K}.textPaths.label`)}
            placeholder="text"
            className="font-mono text-xs"
            onCommit={(text_paths) => commit({ text_paths })}
          />
        </Field>
        <Field
          title={t(`${K}.timeout.label`)}
          description={t(`${K}.timeout.description`)}
        >
          <BlurInput
            type="number"
            min={1}
            max={3600}
            value={String(draft.timeout_secs)}
            aria-label={t(`${K}.timeout.label`)}
            onCommit={(v) => {
              // Emptied or junk: back to the default, not to one second.
              const secs = Math.round(Number(v));
              commit({
                timeout_secs:
                  Number.isFinite(secs) && secs > 0
                    ? secs
                    : DEFAULT_TIMEOUT_SECS,
              });
            }}
          />
        </Field>
      </div>
    </details>
  );
};

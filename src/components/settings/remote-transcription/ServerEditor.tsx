import React, { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Trash2 } from "lucide-react";
import { Button } from "../../ui/Button";
import { Dropdown } from "../../ui/Dropdown";
import { Alert } from "../../ui/Alert";
import type { RemoteBody } from "@/bindings";
import {
  isUnencryptedOverInternet,
  withBody,
  type RemoteServer,
} from "@/lib/remoteServers";
import { ConnectionTest } from "./ConnectionTest";
import { MoreOptions } from "./MoreOptions";
import { PairsEditor } from "./PairsEditor";
import { BlurInput, Field, K, LongTextInput, useErrorText } from "./shared";

/** How a save went: refused with a reason, or what was stored. */
export type SaveResult = { error: string } | { stored: RemoteServer };

const sameServer = (a: RemoteServer, b: RemoteServer) =>
  JSON.stringify(a) === JSON.stringify(b);

/** Deleting takes a second click, so a slip doesn't lose the server. */
const DeleteButton: React.FC<{ onDelete: () => void }> = ({ onDelete }) => {
  const { t } = useTranslation();
  const [armed, setArmed] = useState(false);
  useEffect(() => {
    if (!armed) return;
    const reset = setTimeout(() => setArmed(false), 4000);
    return () => clearTimeout(reset);
  }, [armed]);
  return (
    <Button
      variant={armed ? "danger" : "danger-ghost"}
      size="sm"
      onClick={() => (armed ? onDelete() : setArmed(true))}
    >
      <span className="flex items-center gap-1.5">
        <Trash2 className="size-[13px]" aria-hidden="true" />
        {armed ? t(`${K}.deleteConfirm`) : t(`${K}.delete`)}
      </span>
    </Button>
  );
};

/** Every setting of one server, saved as each field is left. */
export const ServerEditor: React.FC<{
  saved: RemoteServer;
  /** Whether dictation uses this server. */
  inUse: boolean;
  onUse: () => void;
  onSave: (server: RemoteServer) => Promise<SaveResult>;
  onClearKey: () => void;
  onDelete: () => void;
}> = ({ saved, inUse, onUse, onSave, onClearKey, onDelete }) => {
  const { t } = useTranslation();
  const errorText = useErrorText();
  const [draft, setDraft] = useState(saved);
  const [error, setError] = useState<string | null>(null);
  // The latest draft, for edits that land before a re-render.
  const draftRef = useRef(saved);
  const savedRef = useRef(saved);
  savedRef.current = saved;
  // Saves still on their way. While any is, what's stored is older than
  // what's typed, so it isn't copied back over the form.
  const pending = useRef(0);

  const show = (next: RemoteServer) => {
    draftRef.current = next;
    setDraft(next);
  };

  // Follow what's stored when it changes on its own (another window, a
  // removed key), never over edits still being saved.
  const savedKey = JSON.stringify(saved);
  const seenKey = useRef(savedKey);
  useEffect(() => {
    if (seenKey.current === savedKey) return;
    seenKey.current = savedKey;
    if (pending.current > 0) return;
    show(saved);
    setError(null);
  }, [saved, savedKey]);

  const commit = async (patch: Partial<RemoteServer>) => {
    const next = { ...draftRef.current, ...patch };
    show(next);
    // Back to what's stored: nothing to save, and nothing wrong any more.
    if (sameServer(next, savedRef.current)) return setError(null);
    pending.current += 1;
    const result = await onSave(next);
    pending.current -= 1;
    if ("error" in result) return setError(result.error);
    setError(null);
    if (draftRef.current === next) {
      // Nothing typed since: show what was stored (trimmed, key gone).
      show(result.stored);
    } else {
      // Newer edits stay, but take what only the backend knows: whether a
      // key is saved. A key just saved is in the keychain now, so it
      // leaves the form and isn't sent again with the next edit.
      const sentKey = next.api_key && draftRef.current.api_key === next.api_key;
      show({
        ...draftRef.current,
        has_api_key: result.stored.has_api_key,
        api_key: sentKey ? "" : draftRef.current.api_key,
      });
    }
  };

  return (
    <>
      {!inUse && (
        <div className="flex items-center justify-between gap-3 px-4 py-2">
          <span className="text-sm text-ink-2">{t(`${K}.notInUse`)}</span>
          <Button variant="primary-soft" size="sm" onClick={onUse}>
            {t(`${K}.use`)}
          </Button>
        </div>
      )}
      <Field
        title={t(`${K}.name.label`)}
        description={t(`${K}.name.description`)}
      >
        <BlurInput
          value={draft.name}
          aria-label={t(`${K}.name.label`)}
          placeholder={t(`${K}.name.placeholder`)}
          onCommit={(name) => commit({ name })}
        />
      </Field>
      <Field
        title={t(`${K}.url.label`)}
        description={t(`${K}.url.description`)}
      >
        <BlurInput
          value={draft.url}
          aria-label={t(`${K}.url.label`)}
          placeholder="http://192.168.1.10:8000/v1/audio/transcriptions"
          inputMode="url"
          onCommit={(url) => commit({ url })}
        />
      </Field>
      <Field
        title={t(`${K}.body.label`)}
        description={t(`${K}.body.description`)}
      >
        <Dropdown
          options={(["multipart", "chat", "raw", "json"] as RemoteBody[]).map(
            (b) => ({ value: b, label: t(`${K}.body.${b}`) }),
          )}
          selectedValue={draft.body}
          onSelect={(body) =>
            commit(withBody(draftRef.current, body as RemoteBody))
          }
        />
      </Field>
      <Field
        title={t(`${K}.apiKey.label`)}
        description={t(`${K}.apiKey.description`)}
      >
        {/* The key goes to the system keychain and never comes back: the
            field is for entering one, and says when one is saved. */}
        <div className="flex items-center gap-2">
          <BlurInput
            type="password"
            value={draft.api_key}
            aria-label={t(`${K}.apiKey.label`)}
            placeholder={
              saved.has_api_key ? t(`${K}.apiKey.saved`) : t(`${K}.optional`)
            }
            onCommit={(api_key) => commit({ api_key })}
          />
          {saved.has_api_key && (
            <Button variant="ghost" size="sm" onClick={onClearKey}>
              {t(`${K}.apiKey.remove`)}
            </Button>
          )}
        </div>
      </Field>
      {draft.body === "chat" && (
        <Field
          title={t(`${K}.prompt.label`)}
          description={t(`${K}.prompt.description`)}
        >
          <LongTextInput
            label={t(`${K}.prompt.label`)}
            value={draft.prompt}
            rows={4}
            onCommit={(prompt) => commit({ prompt })}
          />
        </Field>
      )}
      {(draft.body === "multipart" || draft.body === "chat") && (
        <Field
          title={t(`${K}.fields.label`)}
          description={t(
            draft.body === "chat"
              ? `${K}.fields.chatDescription`
              : `${K}.fields.description`,
          )}
        >
          <PairsEditor
            pairs={draft.fields}
            kind="fields"
            onChange={(fields) => commit({ fields })}
          />
        </Field>
      )}
      {draft.body === "json" && (
        <Field
          title={t(`${K}.jsonBody.label`)}
          description={t(`${K}.jsonBody.description`)}
        >
          <LongTextInput
            label={t(`${K}.jsonBody.label`)}
            value={draft.json_body}
            rows={8}
            mono
            onCommit={(json_body) => commit({ json_body })}
          />
        </Field>
      )}
      {draft.body === "raw" && (
        <p className="px-4 pb-2 text-xs text-ink-3">{t(`${K}.body.rawHint`)}</p>
      )}

      <MoreOptions draft={draft} commit={commit} />

      {isUnencryptedOverInternet(draft.url) && (
        <div className="px-4 pb-2">
          <Alert variant="warning">{t(`${K}.unencrypted`)}</Alert>
        </div>
      )}
      {error && (
        <div className="px-4 pb-2">
          <Alert variant="error">{errorText(error)}</Alert>
        </div>
      )}
      <ConnectionTest server={draft} />
      <div className="flex justify-end px-4 pb-3">
        <DeleteButton onDelete={onDelete} />
      </div>
    </>
  );
};

import React, { useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { ToggleSwitch } from "../../ui/ToggleSwitch";
import { Dropdown } from "../../ui/Dropdown";
import { Alert } from "../../ui/Alert";
import { useSettings } from "../../../hooks/useSettings";
import { useSettingsStore } from "../../../stores/settingsStore";
import { commands } from "@/bindings";
import {
  REMOTE_SERVER_PRESETS,
  newServerFromPreset,
  withDefaults,
  type RemoteServer,
} from "@/lib/remoteServers";
import { AddServerMenu } from "./AddServerMenu";
import { ServerEditor, type SaveResult } from "./ServerEditor";
import { Field, K, useErrorText, usePresetName } from "./shared";

/**
 * Send recordings to a speech-to-text server instead of running a model on
 * this computer. Servers are described entirely by the user.
 */
export const RemoteTranscription: React.FC = () => {
  const { t } = useTranslation();
  const presetName = usePresetName();
  const errorText = useErrorText();
  // Why the last add, switch or key removal didn't happen.
  const [failed, setFailed] = useState<string | null>(null);
  const report = (
    result: { status: "ok" } | { status: "error"; error: string },
  ) => setFailed(result.status === "error" ? result.error : null);
  const { settings, updateSetting, isUpdating, refreshSettings } =
    useSettings();
  // Every change runs one at a time, in the order it was made: a save
  // waiting on the keychain must not land after a later delete, or bring
  // back what was deleted.
  const queue = useRef<Promise<unknown>>(Promise.resolve());
  const inOrder = <T,>(task: () => Promise<T>): Promise<T> => {
    const run = queue.current.then(task);
    queue.current = run.catch(() => undefined);
    return run;
  };
  const enabled = settings?.remote_transcription_enabled ?? false;
  const stored = settings?.remote_transcription_servers;
  const servers = useMemo(() => (stored ?? []).map(withDefaults), [stored]);
  // The server dictation uses, and the one open in the editor. They differ
  // while the user looks at or sets up another one: nothing is rerouted
  // until they say so.
  const inUseId = settings?.remote_transcription_server_id ?? null;
  const inUse = servers.find((s) => s.id === inUseId) ?? null;
  const [viewId, setViewId] = useState<string | null>(null);
  const viewed =
    servers.find((s) => s.id === viewId) ?? inUse ?? servers[0] ?? null;

  const save = (server: RemoteServer): Promise<SaveResult> =>
    inOrder(() => saveNow(server));

  const saveNow = async (server: RemoteServer): Promise<SaveResult> => {
    const result = await commands.saveRemoteTranscriptionServer(server);
    // A refused save leaves the form as typed, with the reason under it.
    if (result.status === "error") return { error: result.error };
    await refreshSettings();
    // What the backend kept: trimmed, and without the key it moved to the
    // keychain.
    const kept = useSettingsStore
      .getState()
      .settings?.remote_transcription_servers?.find((s) => s.id === server.id);
    return { stored: kept ? withDefaults(kept) : server };
  };

  const add = (presetKey: string) => inOrder(() => addNow(presetKey));

  const addNow = async (presetKey: string) => {
    const preset = REMOTE_SERVER_PRESETS.find((p) => p.key === presetKey);
    if (!preset) return;
    const server = newServerFromPreset(
      { ...preset, name: presetName(preset.key, preset.name) },
      t(`${K}.name.untitled`, { n: servers.length + 1 }),
    );
    const result = await commands.saveRemoteTranscriptionServer(server);
    report(result);
    if (result.status === "error") return;
    setViewId(server.id);
    await refreshSettings();
  };

  const use = (id: string) =>
    inOrder(async () => {
      report(await commands.selectRemoteTranscriptionServer(id));
      await refreshSettings();
    });

  const clearKey = (id: string) =>
    inOrder(async () => {
      report(await commands.clearRemoteTranscriptionApiKey(id));
      await refreshSettings();
    });

  const remove = (id: string) =>
    inOrder(async () => {
      report(await commands.deleteRemoteTranscriptionServer(id));
      setViewId(null);
      await refreshSettings();
    });

  return (
    <>
      <ToggleSwitch
        checked={enabled}
        onChange={(next) => updateSetting("remote_transcription_enabled", next)}
        isUpdating={isUpdating("remote_transcription_enabled")}
        label={t(`${K}.label`)}
        description={t(`${K}.description`)}
        descriptionMode="tooltip"
        grouped={true}
      />
      {enabled && !inUse?.url.trim() && (
        <div className="px-4 pb-2">
          <Alert variant="warning">{t(`${K}.needsServer`)}</Alert>
        </div>
      )}
      {failed && (
        <div className="px-4 pb-2">
          <Alert variant="error">{errorText(failed)}</Alert>
        </div>
      )}
      <Field
        title={t(`${K}.server.label`)}
        description={t(`${K}.server.description`)}
      >
        <div className="flex items-center gap-2">
          {servers.length > 0 ? (
            <Dropdown
              className="min-w-0 flex-1"
              options={servers.map((s) => {
                const name = s.name || s.url || t(`${K}.name.placeholder`);
                return {
                  value: s.id,
                  label:
                    s.id === inUseId ? t(`${K}.server.inUse`, { name }) : name,
                };
              })}
              selectedValue={viewed?.id ?? null}
              placeholder={t(`${K}.server.choose`)}
              onSelect={setViewId}
            />
          ) : (
            <span className="flex-1 text-sm text-ink-3">
              {t(`${K}.server.none`)}
            </span>
          )}
          <AddServerMenu onPick={add} />
        </div>
      </Field>
      {viewed && (
        <ServerEditor
          key={viewed.id}
          saved={viewed}
          inUse={viewed.id === inUseId}
          onUse={() => use(viewed.id)}
          onSave={save}
          onClearKey={() => clearKey(viewed.id)}
          onDelete={() => remove(viewed.id)}
        />
      )}
    </>
  );
};

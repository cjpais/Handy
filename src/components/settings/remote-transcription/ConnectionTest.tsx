import React, { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../ui/Button";
import { Alert } from "../../ui/Alert";
import { commands } from "@/bindings";
import type { RemoteServer } from "@/lib/remoteServers";
import { K, useErrorText } from "./shared";

type TestState =
  | { kind: "idle" }
  | { kind: "testing" }
  | { kind: "ok"; ms: number }
  | { kind: "error"; message: string };

/** Sends a second of silence to `server`, as a dictation would. */
export const ConnectionTest: React.FC<{ server: RemoteServer }> = ({
  server,
}) => {
  const { t } = useTranslation();
  const errorText = useErrorText();
  // What a server or the network said can't be translated; say so around it.
  const failure = (raw: string) => {
    const text = errorText(raw);
    return text === raw ? t(`${K}.test.failed`, { detail: raw }) : text;
  };
  const [state, setState] = useState<TestState>({ kind: "idle" });
  // What the test depends on. A typed key moving to the keychain on save
  // (api_key emptied, has_api_key set) doesn't change the server tested.
  const { api_key: _key, has_api_key: _hasKey, ...config } = server;
  const signature = JSON.stringify(config);
  // The server a result belongs to. A result for an older one is dropped.
  const current = useRef(signature);
  useEffect(() => {
    current.current = signature;
    setState({ kind: "idle" });
  }, [signature]);

  const run = async () => {
    const tested = signature;
    setState({ kind: "testing" });
    const result = await commands.testRemoteTranscription(server);
    if (current.current !== tested) return;
    setState(
      result.status === "ok"
        ? { kind: "ok", ms: result.data.elapsed_ms }
        : { kind: "error", message: failure(result.error) },
    );
  };

  return (
    <div className="space-y-2 px-4 pb-3 pt-2">
      <div className="flex items-center gap-3">
        <Button
          variant="secondary"
          size="sm"
          onClick={run}
          disabled={!server.url.trim() || state.kind === "testing"}
        >
          {state.kind === "testing"
            ? t(`${K}.test.testing`)
            : t(`${K}.test.button`)}
        </Button>
        {state.kind === "idle" && (
          <span className="text-xs text-ink-3">{t(`${K}.test.hint`)}</span>
        )}
      </div>
      <div aria-live="polite">
        {state.kind === "ok" && (
          <Alert variant="success">
            {t(`${K}.test.ok`, { seconds: (state.ms / 1000).toFixed(1) })}
          </Alert>
        )}
        {state.kind === "error" && (
          <Alert variant="error">
            <span className="break-all">{state.message}</span>
          </Alert>
        )}
      </div>
    </div>
  );
};

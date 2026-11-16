import React from "react";
import { useTranslation } from "react-i18next";
import { Cloud } from "lucide-react";
import type { RemoteAsrServer } from "@/bindings";

/** Stands in for the model picker while a remote server transcribes. */
const RemoteServerStatus: React.FC<{ server: RemoteAsrServer }> = ({
  server,
}) => {
  const { t } = useTranslation();
  const host = (() => {
    try {
      return new URL(server.url ?? "").host;
    } catch {
      return server.url ?? "";
    }
  })();
  return (
    <span
      className="flex items-center gap-1.5 text-ink-2"
      title={t("settings.advanced.remoteTranscription.footer", { host })}
    >
      <Cloud className="size-[13px] shrink-0" aria-hidden="true" />
      <span className="max-w-[180px] truncate">{server.name || host}</span>
    </span>
  );
};

export default RemoteServerStatus;

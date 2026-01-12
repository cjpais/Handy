import React, { useEffect, useState, useCallback } from "react";
import { useTranslation } from "react-i18next";
import { commands, TmuxSession, RecoveredSession } from "@/bindings";
import {
  Terminal,
  Play,
  Square,
  RefreshCcw,
  Loader2,
  AlertCircle,
  CheckCircle2,
  Clock,
  Trash2,
  RotateCcw,
  GitBranch,
  ExternalLink,
} from "lucide-react";

interface SessionManagerProps {
  onSessionsChange?: () => void;
}

export const SessionManager: React.FC<SessionManagerProps> = ({
  onSessionsChange,
}) => {
  const { t } = useTranslation();
  const [sessions, setSessions] = useState<TmuxSession[]>([]);
  const [recoveredSessions, setRecoveredSessions] = useState<RecoveredSession[]>([]);
  const [isLoading, setIsLoading] = useState(true);
  const [isTmuxRunning, setIsTmuxRunning] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [killingSession, setKillingSession] = useState<string | null>(null);

  const loadSessions = useCallback(async () => {
    setIsLoading(true);
    setError(null);
    try {
      const running = await commands.isTmuxRunning();
      setIsTmuxRunning(running);

      if (running) {
        const [sessionList, recovered] = await Promise.all([
          commands.listTmuxSessions(),
          commands.recoverTmuxSessions(),
        ]);
        setSessions(sessionList);
        setRecoveredSessions(recovered);
      } else {
        setSessions([]);
        setRecoveredSessions([]);
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setIsLoading(false);
    }
  }, []);

  useEffect(() => {
    loadSessions();
    // Refresh every 10 seconds
    const interval = setInterval(loadSessions, 10000);
    return () => clearInterval(interval);
  }, [loadSessions]);

  const handleKillSession = async (sessionName: string) => {
    setKillingSession(sessionName);
    try {
      await commands.killTmuxSession(sessionName);
      await loadSessions();
      onSessionsChange?.();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setKillingSession(null);
    }
  };

  const formatTimestamp = (timestamp: string) => {
    try {
      const date = new Date(timestamp);
      return date.toLocaleString();
    } catch {
      return timestamp;
    }
  };

  const getStatusIcon = (status: string) => {
    switch (status) {
      case "Running":
        return <Play className="w-4 h-4 text-green-400" />;
      case "Stopped":
        return <Square className="w-4 h-4 text-yellow-400" />;
      default:
        return <AlertCircle className="w-4 h-4 text-gray-400" />;
    }
  };

  if (isLoading) {
    return (
      <div className="flex items-center justify-center p-8">
        <Loader2 className="w-6 h-6 animate-spin text-logo-primary" />
      </div>
    );
  }

  if (!isTmuxRunning) {
    return (
      <div className="flex flex-col items-center justify-center p-8 text-center">
        <Terminal className="w-12 h-12 text-mid-gray/50 mb-3" />
        <p className="text-sm text-mid-gray mb-2">
          {t("devops.sessions.tmuxNotRunning")}
        </p>
        <p className="text-xs text-mid-gray/70">
          {t("devops.sessions.tmuxNotRunningHint")}
        </p>
      </div>
    );
  }

  if (error) {
    return (
      <div className="flex items-center gap-2 p-3 bg-red-500/10 rounded-lg text-red-400">
        <AlertCircle className="w-4 h-4" />
        <span className="text-sm">{error}</span>
        <button
          onClick={loadSessions}
          className="ml-auto p-1 hover:bg-mid-gray/20 rounded"
        >
          <RefreshCcw className="w-4 h-4" />
        </button>
      </div>
    );
  }

  if (sessions.length === 0) {
    return (
      <div className="flex flex-col items-center justify-center p-8 text-center">
        <Terminal className="w-12 h-12 text-mid-gray/50 mb-3" />
        <p className="text-sm text-mid-gray">{t("devops.sessions.noSessions")}</p>
        <p className="text-xs text-mid-gray/70 mt-1">
          {t("devops.sessions.noSessionsHint")}
        </p>
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-3">
      {/* Header with refresh */}
      <div className="flex items-center justify-between">
        <span className="text-sm text-mid-gray">
          {t("devops.sessions.activeCount", { count: sessions.length })}
        </span>
        <button
          onClick={loadSessions}
          disabled={isLoading}
          className="p-1 hover:bg-mid-gray/20 rounded transition-colors"
          title={t("devops.refresh")}
        >
          <RefreshCcw className={`w-4 h-4 ${isLoading ? "animate-spin" : ""}`} />
        </button>
      </div>

      {/* Session list */}
      <div className="flex flex-col gap-2">
        {sessions.map((session) => (
          <div
            key={session.name}
            className="flex items-start gap-3 p-3 rounded-lg bg-mid-gray/10 hover:bg-mid-gray/15 transition-colors"
          >
            {/* Status icon */}
            <div className="mt-1">{getStatusIcon(session.status)}</div>

            {/* Content */}
            <div className="flex-1 min-w-0">
              <div className="flex items-center gap-2">
                <code className="font-medium text-sm">{session.name}</code>
                {session.attached && (
                  <span className="text-xs px-1.5 py-0.5 rounded bg-green-500/20 text-green-400">
                    {t("devops.sessions.attached")}
                  </span>
                )}
              </div>

              {session.metadata && (
                <div className="mt-1 text-xs text-mid-gray space-y-0.5">
                  {session.metadata.issue_ref && (
                    <div className="flex items-center gap-1">
                      <GitBranch className="w-3 h-3" />
                      <span>{session.metadata.issue_ref}</span>
                    </div>
                  )}
                  {session.metadata.agent_type && (
                    <div className="flex items-center gap-1">
                      <Terminal className="w-3 h-3" />
                      <span>{session.metadata.agent_type}</span>
                    </div>
                  )}
                  {session.metadata.started_at && (
                    <div className="flex items-center gap-1">
                      <Clock className="w-3 h-3" />
                      <span>{formatTimestamp(session.metadata.started_at)}</span>
                    </div>
                  )}
                </div>
              )}
            </div>

            {/* Actions */}
            <div className="flex items-center gap-1">
              <button
                onClick={() => handleKillSession(session.name)}
                disabled={killingSession === session.name}
                className="p-1.5 hover:bg-red-500/20 rounded transition-colors text-red-400"
                title={t("devops.sessions.kill")}
              >
                {killingSession === session.name ? (
                  <Loader2 className="w-4 h-4 animate-spin" />
                ) : (
                  <Trash2 className="w-4 h-4" />
                )}
              </button>
            </div>
          </div>
        ))}
      </div>

      {/* Recovery suggestions */}
      {recoveredSessions.some((s) => s.recommended_action !== "None") && (
        <div className="mt-4 pt-4 border-t border-mid-gray/20">
          <h4 className="text-sm font-medium mb-2">
            {t("devops.sessions.recoveryTitle")}
          </h4>
          <div className="flex flex-col gap-2">
            {recoveredSessions
              .filter((s) => s.recommended_action !== "None")
              .map((session) => (
                <div
                  key={session.metadata.session}
                  className="flex items-center gap-2 p-2 rounded bg-yellow-500/10 text-sm"
                >
                  <RotateCcw className="w-4 h-4 text-yellow-400" />
                  <span className="flex-1">
                    {session.metadata.session}: {t(`devops.sessions.action.${session.recommended_action.toLowerCase()}`)}
                  </span>
                </div>
              ))}
          </div>
        </div>
      )}
    </div>
  );
};

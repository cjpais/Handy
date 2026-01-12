import React, { useEffect, useState, useCallback } from "react";
import { useTranslation } from "react-i18next";
import { commands, AgentStatus } from "@/bindings";
import {
  Bot,
  RefreshCcw,
  Loader2,
  AlertCircle,
  ExternalLink,
  Trash2,
  GitPullRequest,
  CircleDot,
  FolderGit2,
  Terminal,
  Monitor,
  Clock,
  CheckCircle2,
  Filter,
  Laptop,
  Globe,
} from "lucide-react";

interface AgentDashboardProps {
  onAgentSelect?: (agent: AgentStatus) => void;
  repoPath?: string;
}

export const AgentDashboard: React.FC<AgentDashboardProps> = ({
  onAgentSelect,
  repoPath,
}) => {
  const { t } = useTranslation();
  const [agents, setAgents] = useState<AgentStatus[]>([]);
  const [isLoading, setIsLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [cleaningUp, setCleaningUp] = useState<string | null>(null);
  const [completingWork, setCompletingWork] = useState<string | null>(null);
  const [filterMode, setFilterMode] = useState<"all" | "local" | "remote">("all");
  const [currentMachineId, setCurrentMachineId] = useState<string>("");

  const loadAgents = useCallback(async () => {
    setIsLoading(true);
    setError(null);
    try {
      const statuses = await commands.listAgentStatuses();
      setAgents(statuses);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setIsLoading(false);
    }
  }, []);

  useEffect(() => {
    loadAgents();
    // Load current machine ID
    commands.getCurrentMachineId().then(setCurrentMachineId).catch(() => {});
    // Refresh every 10 seconds
    const interval = setInterval(loadAgents, 10000);
    return () => clearInterval(interval);
  }, [loadAgents]);

  const handleCompleteWork = async (agent: AgentStatus) => {
    if (!agent.issue_ref) {
      setError(t("devops.orchestrator.noIssueRef"));
      return;
    }

    setCompletingWork(agent.session);
    setError(null);

    try {
      const prTitle = `Fix for ${agent.issue_ref}`;
      await commands.completeAgentWork(
        agent.session,
        prTitle,
        null,
        ["agent-working"], // Labels to remove
        ["needs-review"], // Labels to add
        false // Not draft
      );
      await loadAgents();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setCompletingWork(null);
    }
  };

  // Filter agents based on filter mode
  const filteredAgents = agents.filter((agent) => {
    if (filterMode === "local") return agent.is_local;
    if (filterMode === "remote") return !agent.is_local;
    return true;
  });

  const handleCleanup = async (agent: AgentStatus, removeWorktree: boolean) => {
    if (!agent.worktree) {
      setError("Agent has no associated worktree");
      return;
    }

    setCleaningUp(agent.session);
    try {
      // Get repo root from worktree path
      const repoRoot = await commands.getGitRepoRoot(agent.worktree);
      await commands.cleanupAgent(agent.session, repoRoot, removeWorktree, removeWorktree);
      await loadAgents();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setCleaningUp(null);
    }
  };

  const formatDate = (dateStr: string) => {
    if (dateStr === "unknown") return dateStr;
    try {
      const date = new Date(dateStr);
      return date.toLocaleString();
    } catch {
      return dateStr;
    }
  };

  const currentMachineId = typeof window !== "undefined" ? window.navigator.userAgent.slice(0, 20) : "unknown";

  if (isLoading && agents.length === 0) {
    return (
      <div className="flex items-center justify-center p-8">
        <Loader2 className="w-6 h-6 animate-spin text-logo-primary" />
      </div>
    );
  }

  if (error) {
    return (
      <div className="flex items-center gap-2 p-3 bg-red-500/10 rounded-lg text-red-400">
        <AlertCircle className="w-4 h-4" />
        <span className="text-sm">{error}</span>
        <button
          onClick={loadAgents}
          className="ml-auto p-1 hover:bg-mid-gray/20 rounded"
        >
          <RefreshCcw className="w-4 h-4" />
        </button>
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-3">
      {/* Header */}
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <span className="text-sm text-mid-gray">
            {t("devops.orchestrator.agentCount", { count: filteredAgents.length })}
          </span>
          {currentMachineId && (
            <span className="text-xs text-mid-gray/50 flex items-center gap-1">
              <Laptop className="w-3 h-3" />
              {currentMachineId.slice(0, 12)}
            </span>
          )}
        </div>
        <div className="flex items-center gap-2">
          {/* Filter buttons */}
          <div className="flex items-center rounded bg-mid-gray/10">
            <button
              onClick={() => setFilterMode("all")}
              className={`px-2 py-1 text-xs rounded-l transition-colors ${
                filterMode === "all"
                  ? "bg-logo-primary text-white"
                  : "text-mid-gray hover:text-white"
              }`}
              title={t("devops.orchestrator.filterAll")}
            >
              {t("devops.orchestrator.all")}
            </button>
            <button
              onClick={() => setFilterMode("local")}
              className={`px-2 py-1 text-xs transition-colors ${
                filterMode === "local"
                  ? "bg-logo-primary text-white"
                  : "text-mid-gray hover:text-white"
              }`}
              title={t("devops.orchestrator.filterLocal")}
            >
              <Laptop className="w-3 h-3" />
            </button>
            <button
              onClick={() => setFilterMode("remote")}
              className={`px-2 py-1 text-xs rounded-r transition-colors ${
                filterMode === "remote"
                  ? "bg-logo-primary text-white"
                  : "text-mid-gray hover:text-white"
              }`}
              title={t("devops.orchestrator.filterRemote")}
            >
              <Globe className="w-3 h-3" />
            </button>
          </div>
          <button
            onClick={loadAgents}
            disabled={isLoading}
            className="p-1 hover:bg-mid-gray/20 rounded transition-colors"
            title={t("devops.refresh")}
          >
            <RefreshCcw className={`w-4 h-4 ${isLoading ? "animate-spin" : ""}`} />
          </button>
        </div>
      </div>

      {/* Agent list */}
      {filteredAgents.length === 0 ? (
        <div className="flex flex-col items-center justify-center p-8 text-center">
          <Bot className="w-12 h-12 text-mid-gray/50 mb-3" />
          <p className="text-sm text-mid-gray">
            {filterMode === "all"
              ? t("devops.orchestrator.noAgents")
              : filterMode === "local"
                ? t("devops.orchestrator.noLocalAgents")
                : t("devops.orchestrator.noRemoteAgents")}
          </p>
          <p className="text-xs text-mid-gray/70 mt-1">
            {t("devops.orchestrator.noAgentsHint")}
          </p>
        </div>
      ) : (
        <div className="flex flex-col gap-2 max-h-[400px] overflow-y-auto">
          {filteredAgents.map((agent) => (
            <div
              key={agent.session}
              className="flex flex-col rounded-lg bg-mid-gray/10 hover:bg-mid-gray/15 transition-colors"
            >
              {/* Agent Header */}
              <div
                className="flex items-start gap-3 p-3 cursor-pointer"
                onClick={() => onAgentSelect?.(agent)}
              >
                {/* Status icon */}
                <div className="mt-1">
                  <Bot className={`w-4 h-4 ${agent.is_attached ? "text-green-400" : "text-mid-gray"}`} />
                </div>

                {/* Content */}
                <div className="flex-1 min-w-0">
                  <div className="flex items-center gap-2">
                    <span className="font-medium text-sm truncate">
                      {agent.session}
                    </span>
                    {agent.is_attached && (
                      <span className="text-xs px-1.5 py-0.5 rounded bg-green-500/20 text-green-400">
                        {t("devops.orchestrator.attached")}
                      </span>
                    )}
                    {agent.is_local ? (
                      <span className="text-xs px-1.5 py-0.5 rounded bg-blue-500/20 text-blue-400 flex items-center gap-1">
                        <Laptop className="w-3 h-3" />
                        {t("devops.orchestrator.local")}
                      </span>
                    ) : (
                      <span className="text-xs px-1.5 py-0.5 rounded bg-yellow-500/20 text-yellow-400 flex items-center gap-1">
                        <Globe className="w-3 h-3" />
                        {t("devops.orchestrator.remote")}
                      </span>
                    )}
                  </div>

                  {/* Issue info */}
                  {agent.issue_ref && (
                    <div className="mt-1 flex items-center gap-2 text-xs">
                      <CircleDot className="w-3 h-3 text-green-400" />
                      <span className="text-mid-gray">{agent.issue_ref}</span>
                    </div>
                  )}

                  {/* Metadata */}
                  <div className="mt-2 flex flex-wrap gap-3 text-xs text-mid-gray/70">
                    {agent.worktree && (
                      <span className="flex items-center gap-1" title={agent.worktree}>
                        <FolderGit2 className="w-3 h-3" />
                        {agent.worktree.split("/").pop()}
                      </span>
                    )}
                    <span className="flex items-center gap-1">
                      <Terminal className="w-3 h-3" />
                      {agent.agent_type}
                    </span>
                    <span className="flex items-center gap-1">
                      <Monitor className="w-3 h-3" />
                      {agent.machine_id.slice(0, 12)}
                    </span>
                    <span className="flex items-center gap-1">
                      <Clock className="w-3 h-3" />
                      {formatDate(agent.started_at)}
                    </span>
                  </div>
                </div>

                {/* Actions */}
                <div className="flex items-center gap-1">
                  {/* Complete Work button - only for local agents with issue */}
                  {agent.is_local && agent.issue_ref && (
                    <button
                      onClick={(e) => {
                        e.stopPropagation();
                        handleCompleteWork(agent);
                      }}
                      disabled={completingWork === agent.session}
                      className="p-1.5 hover:bg-green-500/20 rounded transition-colors text-mid-gray hover:text-green-400"
                      title={t("devops.orchestrator.completeWork")}
                    >
                      {completingWork === agent.session ? (
                        <Loader2 className="w-4 h-4 animate-spin" />
                      ) : (
                        <GitPullRequest className="w-4 h-4" />
                      )}
                    </button>
                  )}
                  <button
                    onClick={(e) => {
                      e.stopPropagation();
                      handleCleanup(agent, true);
                    }}
                    disabled={cleaningUp === agent.session || !agent.is_local}
                    className="p-1.5 hover:bg-red-500/20 rounded transition-colors text-mid-gray hover:text-red-400 disabled:opacity-50"
                    title={agent.is_local ? t("devops.orchestrator.cleanup") : t("devops.orchestrator.remoteCannotCleanup")}
                  >
                    {cleaningUp === agent.session ? (
                      <Loader2 className="w-4 h-4 animate-spin" />
                    ) : (
                      <Trash2 className="w-4 h-4" />
                    )}
                  </button>
                </div>
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  );
};

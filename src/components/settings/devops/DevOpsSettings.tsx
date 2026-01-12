import React, { useEffect, useState, useCallback } from "react";
import { useTranslation } from "react-i18next";
import { commands, DevOpsDependencies } from "@/bindings";
import { SettingsGroup } from "../../ui/SettingsGroup";
import { DependencyStatus } from "./DependencyStatus";
import { SessionManager } from "./SessionManager";
import { WorktreeManager } from "./WorktreeManager";
import { IssueQueue } from "./IssueQueue";
import { PullRequestPanel } from "./PullRequestPanel";
import { AgentDashboard } from "./AgentDashboard";
import {
  Terminal,
  GitBranch,
  GitPullRequest,
  FolderGit2,
  CircleDot,
  RefreshCcw,
  Loader2,
  AlertCircle,
  CheckCircle2,
  Bot,
} from "lucide-react";

export const DevOpsSettings: React.FC = () => {
  const { t } = useTranslation();
  const [dependencies, setDependencies] = useState<DevOpsDependencies | null>(null);
  const [isLoading, setIsLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const checkDependencies = useCallback(async () => {
    setIsLoading(true);
    setError(null);
    try {
      const deps = await commands.checkDevopsDependencies();
      setDependencies(deps);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setIsLoading(false);
    }
  }, []);

  useEffect(() => {
    checkDependencies();
  }, [checkDependencies]);

  return (
    <div className="flex flex-col gap-4">
      {/* Header */}
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <Terminal className="w-5 h-5 text-logo-primary" />
          <h2 className="text-lg font-semibold">{t("devops.title")}</h2>
        </div>
        <button
          onClick={checkDependencies}
          disabled={isLoading}
          className="flex items-center gap-1 px-2 py-1 text-sm rounded hover:bg-mid-gray/20 transition-colors disabled:opacity-50"
        >
          {isLoading ? (
            <Loader2 className="w-4 h-4 animate-spin" />
          ) : (
            <RefreshCcw className="w-4 h-4" />
          )}
          {t("devops.refresh")}
        </button>
      </div>

      {/* Description */}
      <p className="text-sm text-mid-gray">{t("devops.description")}</p>

      {/* Error state */}
      {error && (
        <div className="flex items-center gap-2 p-3 bg-red-500/10 rounded-lg text-red-400">
          <AlertCircle className="w-4 h-4" />
          <span className="text-sm">{error}</span>
        </div>
      )}

      {/* Dependencies Section */}
      <SettingsGroup
        title={t("devops.dependencies.title")}
        description={t("devops.dependencies.description")}
      >
        {isLoading ? (
          <div className="flex items-center justify-center p-4">
            <Loader2 className="w-6 h-6 animate-spin text-logo-primary" />
          </div>
        ) : dependencies ? (
          <div className="flex flex-col gap-3">
            {/* Overall status */}
            <div className="flex items-center gap-2 pb-3 border-b border-mid-gray/20">
              {dependencies.all_satisfied ? (
                <>
                  <CheckCircle2 className="w-5 h-5 text-green-400" />
                  <span className="text-sm text-green-400">
                    {t("devops.dependencies.allSatisfied")}
                  </span>
                </>
              ) : (
                <>
                  <AlertCircle className="w-5 h-5 text-yellow-400" />
                  <span className="text-sm text-yellow-400">
                    {t("devops.dependencies.missing")}
                  </span>
                </>
              )}
            </div>

            {/* Individual dependencies */}
            <DependencyStatus
              name="gh"
              displayName="GitHub CLI"
              icon={<GitBranch className="w-4 h-4" />}
              status={dependencies.gh}
            />
            <DependencyStatus
              name="tmux"
              displayName="tmux"
              icon={<Terminal className="w-4 h-4" />}
              status={dependencies.tmux}
            />
          </div>
        ) : null}
      </SettingsGroup>

      {/* Active Agents Dashboard */}
      {dependencies?.all_satisfied && (
        <SettingsGroup
          title={t("devops.orchestrator.title")}
          description={t("devops.orchestrator.description")}
        >
          <AgentDashboard />
        </SettingsGroup>
      )}

      {/* Agent Sessions */}
      {dependencies?.all_satisfied && (
        <SettingsGroup
          title={t("devops.sessions.title")}
          description={t("devops.sessions.description")}
        >
          <SessionManager onSessionsChange={checkDependencies} />
        </SettingsGroup>
      )}

      {/* Git Worktrees */}
      {dependencies?.all_satisfied && (
        <SettingsGroup
          title={t("devops.worktrees.title")}
          description={t("devops.worktrees.description")}
        >
          <WorktreeManager />
        </SettingsGroup>
      )}

      {/* GitHub Issues */}
      {dependencies?.gh?.installed && (
        <SettingsGroup
          title={t("devops.issues.title")}
          description={t("devops.issues.description")}
        >
          <IssueQueue />
        </SettingsGroup>
      )}

      {/* GitHub Pull Requests */}
      {dependencies?.gh?.installed && (
        <SettingsGroup
          title={t("devops.prs.title")}
          description={t("devops.prs.description")}
        >
          <PullRequestPanel />
        </SettingsGroup>
      )}
    </div>
  );
};

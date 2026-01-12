import React, { useEffect, useState, useCallback } from "react";
import { useTranslation } from "react-i18next";
import { commands, GitHubIssue, GhAuthStatus } from "@/bindings";
import {
  CircleDot,
  ExternalLink,
  RefreshCcw,
  Loader2,
  AlertCircle,
  CheckCircle2,
  Tag,
  User,
  Clock,
  Plus,
  X,
  LogIn,
  Play,
  Bot,
} from "lucide-react";

interface IssueQueueProps {
  hubRepo?: string;
  repoPath?: string;
  onIssueSelect?: (issue: GitHubIssue) => void;
  onAgentSpawned?: () => void;
}

export const IssueQueue: React.FC<IssueQueueProps> = ({
  hubRepo,
  repoPath,
  onIssueSelect,
  onAgentSpawned,
}) => {
  const { t } = useTranslation();
  const [authStatus, setAuthStatus] = useState<GhAuthStatus | null>(null);
  const [issues, setIssues] = useState<GitHubIssue[]>([]);
  const [isLoading, setIsLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [repoInput, setRepoInput] = useState(hubRepo || "");
  const [activeRepo, setActiveRepo] = useState(hubRepo || "");
  const [showRepoInput, setShowRepoInput] = useState(!hubRepo);
  const [spawningIssue, setSpawningIssue] = useState<number | null>(null);

  const checkAuth = useCallback(async () => {
    try {
      const status = await commands.checkGhAuth();
      setAuthStatus(status);
      return status.authenticated;
    } catch (err) {
      setAuthStatus({
        authenticated: false,
        username: null,
        scopes: [],
        error: err instanceof Error ? err.message : String(err),
      });
      return false;
    }
  }, []);

  const loadIssues = useCallback(async () => {
    if (!activeRepo) {
      setIssues([]);
      setIsLoading(false);
      return;
    }

    setIsLoading(true);
    setError(null);

    try {
      const isAuthed = await checkAuth();
      if (!isAuthed) {
        setIsLoading(false);
        return;
      }

      const issueList = await commands.listGithubIssues(
        activeRepo,
        "open",
        null, // all labels
        50 // limit
      );
      setIssues(issueList);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setIsLoading(false);
    }
  }, [activeRepo, checkAuth]);

  useEffect(() => {
    loadIssues();
  }, [loadIssues]);

  const handleSetRepo = () => {
    if (repoInput.trim()) {
      setActiveRepo(repoInput.trim());
      setShowRepoInput(false);
    }
  };

  const formatDate = (dateStr: string) => {
    try {
      const date = new Date(dateStr);
      return date.toLocaleDateString();
    } catch {
      return dateStr;
    }
  };

  const handleSpawnAgent = async (issue: GitHubIssue) => {
    if (!repoPath) {
      setError(t("devops.orchestrator.noRepoPath"));
      return;
    }

    setSpawningIssue(issue.number);
    setError(null);

    try {
      await commands.spawnAgent(
        activeRepo,
        issue.number,
        "claude", // Default agent type
        repoPath,
        null, // Auto-generate session name
        null, // Default prefix
        ["agent-working"] // Add working label
      );
      onAgentSpawned?.();
      await loadIssues(); // Refresh to show updated labels
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setSpawningIssue(null);
    }
  };

  // Not authenticated
  if (authStatus && !authStatus.authenticated) {
    return (
      <div className="flex flex-col items-center justify-center p-8 text-center">
        <LogIn className="w-12 h-12 text-mid-gray/50 mb-3" />
        <p className="text-sm text-mid-gray mb-2">
          {t("devops.issues.notAuthenticated")}
        </p>
        <p className="text-xs text-mid-gray/70 mb-4">
          {t("devops.issues.notAuthenticatedHint")}
        </p>
        <code className="text-xs bg-mid-gray/20 px-3 py-2 rounded">
          gh auth login
        </code>
      </div>
    );
  }

  // No repo configured
  if (!activeRepo || showRepoInput) {
    return (
      <div className="flex flex-col gap-3">
        <p className="text-sm text-mid-gray">
          {t("devops.issues.configureRepo")}
        </p>
        <div className="flex gap-2">
          <input
            type="text"
            value={repoInput}
            onChange={(e) => setRepoInput(e.target.value)}
            placeholder={t("devops.issues.repoPlaceholder")}
            className="flex-1 px-3 py-2 rounded bg-mid-gray/10 border border-mid-gray/20 text-sm focus:outline-none focus:border-logo-primary"
            onKeyDown={(e) => {
              if (e.key === "Enter" && repoInput.trim()) {
                handleSetRepo();
              }
            }}
          />
          <button
            onClick={handleSetRepo}
            disabled={!repoInput.trim()}
            className="px-3 py-2 rounded bg-logo-primary hover:bg-logo-primary/90 disabled:opacity-50 text-sm"
          >
            {t("devops.issues.setRepo")}
          </button>
        </div>
      </div>
    );
  }

  if (isLoading) {
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
          onClick={loadIssues}
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
            {t("devops.issues.count", { count: issues.length })}
          </span>
          <button
            onClick={() => setShowRepoInput(true)}
            className="text-xs text-mid-gray/70 hover:text-mid-gray"
            title={activeRepo}
          >
            ({activeRepo})
          </button>
        </div>
        <button
          onClick={loadIssues}
          disabled={isLoading}
          className="p-1 hover:bg-mid-gray/20 rounded transition-colors"
          title={t("devops.refresh")}
        >
          <RefreshCcw className={`w-4 h-4 ${isLoading ? "animate-spin" : ""}`} />
        </button>
      </div>

      {/* Auth status */}
      {authStatus?.username && (
        <div className="flex items-center gap-2 text-xs text-green-400">
          <CheckCircle2 className="w-3 h-3" />
          <span>{t("devops.issues.authenticatedAs", { user: authStatus.username })}</span>
        </div>
      )}

      {/* Issue list */}
      {issues.length === 0 ? (
        <div className="flex flex-col items-center justify-center p-8 text-center">
          <CircleDot className="w-12 h-12 text-mid-gray/50 mb-3" />
          <p className="text-sm text-mid-gray">
            {t("devops.issues.noIssues")}
          </p>
          <p className="text-xs text-mid-gray/70 mt-1">
            {t("devops.issues.noIssuesHint")}
          </p>
        </div>
      ) : (
        <div className="flex flex-col gap-2 max-h-[400px] overflow-y-auto">
          {issues.map((issue) => (
            <div
              key={`${issue.repo}-${issue.number}`}
              className="flex items-start gap-3 p-3 rounded-lg bg-mid-gray/10 hover:bg-mid-gray/15 transition-colors cursor-pointer"
              onClick={() => onIssueSelect?.(issue)}
            >
              {/* Status icon */}
              <div className="mt-1">
                <CircleDot
                  className={`w-4 h-4 ${
                    issue.state === "open" ? "text-green-400" : "text-purple-400"
                  }`}
                />
              </div>

              {/* Content */}
              <div className="flex-1 min-w-0">
                <div className="flex items-center gap-2">
                  <span className="text-xs text-mid-gray/70">
                    #{issue.number}
                  </span>
                  <span className="font-medium text-sm truncate">
                    {issue.title}
                  </span>
                </div>

                <div className="mt-1 flex flex-wrap gap-2">
                  {issue.labels.slice(0, 3).map((label) => (
                    <span
                      key={label}
                      className="inline-flex items-center gap-1 text-xs px-1.5 py-0.5 rounded bg-mid-gray/20"
                    >
                      <Tag className="w-3 h-3" />
                      {label}
                    </span>
                  ))}
                  {issue.labels.length > 3 && (
                    <span className="text-xs text-mid-gray/70">
                      +{issue.labels.length - 3}
                    </span>
                  )}
                </div>

                <div className="mt-1 flex items-center gap-3 text-xs text-mid-gray/70">
                  <span className="flex items-center gap-1">
                    <User className="w-3 h-3" />
                    {issue.author}
                  </span>
                  <span className="flex items-center gap-1">
                    <Clock className="w-3 h-3" />
                    {formatDate(issue.created_at)}
                  </span>
                </div>
              </div>

              {/* Actions */}
              <div className="flex items-center gap-1">
                {repoPath && (
                  <button
                    onClick={(e) => {
                      e.stopPropagation();
                      handleSpawnAgent(issue);
                    }}
                    disabled={spawningIssue === issue.number}
                    className="p-1.5 hover:bg-green-500/20 rounded transition-colors text-mid-gray hover:text-green-400"
                    title={t("devops.orchestrator.spawnAgent")}
                  >
                    {spawningIssue === issue.number ? (
                      <Loader2 className="w-4 h-4 animate-spin" />
                    ) : (
                      <Bot className="w-4 h-4" />
                    )}
                  </button>
                )}
                <a
                  href={issue.url}
                  target="_blank"
                  rel="noopener noreferrer"
                  onClick={(e) => e.stopPropagation()}
                  className="p-1.5 hover:bg-mid-gray/20 rounded transition-colors text-mid-gray"
                  title={t("devops.issues.openInGitHub")}
                >
                  <ExternalLink className="w-4 h-4" />
                </a>
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  );
};

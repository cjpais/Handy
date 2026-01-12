import React, { useEffect, useState, useCallback } from "react";
import { useTranslation } from "react-i18next";
import { commands, GitHubPullRequest, PrStatus, GhAuthStatus } from "@/bindings";
import {
  GitPullRequest,
  ExternalLink,
  RefreshCcw,
  Loader2,
  AlertCircle,
  CheckCircle2,
  XCircle,
  Clock,
  GitMerge,
  Tag,
  User,
  LogIn,
  ChevronDown,
  ChevronUp,
} from "lucide-react";

interface PullRequestPanelProps {
  hubRepo?: string;
  onPrSelect?: (pr: GitHubPullRequest) => void;
}

export const PullRequestPanel: React.FC<PullRequestPanelProps> = ({
  hubRepo,
  onPrSelect,
}) => {
  const { t } = useTranslation();
  const [authStatus, setAuthStatus] = useState<GhAuthStatus | null>(null);
  const [prs, setPrs] = useState<GitHubPullRequest[]>([]);
  const [prStatuses, setPrStatuses] = useState<Record<number, PrStatus>>({});
  const [isLoading, setIsLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [repoInput, setRepoInput] = useState(hubRepo || "");
  const [activeRepo, setActiveRepo] = useState(hubRepo || "");
  const [showRepoInput, setShowRepoInput] = useState(!hubRepo);
  const [expandedPr, setExpandedPr] = useState<number | null>(null);

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

  const loadPrs = useCallback(async () => {
    if (!activeRepo) {
      setPrs([]);
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

      const prList = await commands.listGithubPrs(
        activeRepo,
        "open",
        null, // all base branches
        50 // limit
      );
      setPrs(prList);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setIsLoading(false);
    }
  }, [activeRepo, checkAuth]);

  const loadPrStatus = useCallback(async (prNumber: number) => {
    if (!activeRepo) return;
    try {
      const status = await commands.getGithubPrStatus(activeRepo, prNumber);
      setPrStatuses(prev => ({ ...prev, [prNumber]: status }));
    } catch (err) {
      console.error("Failed to load PR status:", err);
    }
  }, [activeRepo]);

  useEffect(() => {
    loadPrs();
  }, [loadPrs]);

  const handleSetRepo = () => {
    if (repoInput.trim()) {
      setActiveRepo(repoInput.trim());
      setShowRepoInput(false);
    }
  };

  const handleExpandPr = (prNumber: number) => {
    if (expandedPr === prNumber) {
      setExpandedPr(null);
    } else {
      setExpandedPr(prNumber);
      if (!prStatuses[prNumber]) {
        loadPrStatus(prNumber);
      }
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

  const getCheckStatusIcon = (state: string) => {
    switch (state) {
      case "success":
        return <CheckCircle2 className="w-4 h-4 text-green-400" />;
      case "failure":
        return <XCircle className="w-4 h-4 text-red-400" />;
      case "pending":
        return <Clock className="w-4 h-4 text-yellow-400" />;
      default:
        return <AlertCircle className="w-4 h-4 text-mid-gray" />;
    }
  };

  // Not authenticated
  if (authStatus && !authStatus.authenticated) {
    return (
      <div className="flex flex-col items-center justify-center p-8 text-center">
        <LogIn className="w-12 h-12 text-mid-gray/50 mb-3" />
        <p className="text-sm text-mid-gray mb-2">
          {t("devops.prs.notAuthenticated")}
        </p>
        <p className="text-xs text-mid-gray/70 mb-4">
          {t("devops.prs.notAuthenticatedHint")}
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
          {t("devops.prs.configureRepo")}
        </p>
        <div className="flex gap-2">
          <input
            type="text"
            value={repoInput}
            onChange={(e) => setRepoInput(e.target.value)}
            placeholder={t("devops.prs.repoPlaceholder")}
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
            {t("devops.prs.setRepo")}
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
          onClick={loadPrs}
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
            {t("devops.prs.count", { count: prs.length })}
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
          onClick={loadPrs}
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
          <span>{t("devops.prs.authenticatedAs", { user: authStatus.username })}</span>
        </div>
      )}

      {/* PR list */}
      {prs.length === 0 ? (
        <div className="flex flex-col items-center justify-center p-8 text-center">
          <GitPullRequest className="w-12 h-12 text-mid-gray/50 mb-3" />
          <p className="text-sm text-mid-gray">
            {t("devops.prs.noPrs")}
          </p>
          <p className="text-xs text-mid-gray/70 mt-1">
            {t("devops.prs.noPrsHint")}
          </p>
        </div>
      ) : (
        <div className="flex flex-col gap-2 max-h-[400px] overflow-y-auto">
          {prs.map((pr) => (
            <div
              key={`${pr.repo}-${pr.number}`}
              className="flex flex-col rounded-lg bg-mid-gray/10 hover:bg-mid-gray/15 transition-colors"
            >
              {/* PR Header */}
              <div
                className="flex items-start gap-3 p-3 cursor-pointer"
                onClick={() => onPrSelect?.(pr)}
              >
                {/* Status icon */}
                <div className="mt-1">
                  {pr.state === "MERGED" ? (
                    <GitMerge className="w-4 h-4 text-purple-400" />
                  ) : pr.is_draft ? (
                    <GitPullRequest className="w-4 h-4 text-mid-gray" />
                  ) : (
                    <GitPullRequest className="w-4 h-4 text-green-400" />
                  )}
                </div>

                {/* Content */}
                <div className="flex-1 min-w-0">
                  <div className="flex items-center gap-2">
                    <span className="text-xs text-mid-gray/70">
                      #{pr.number}
                    </span>
                    <span className="font-medium text-sm truncate">
                      {pr.title}
                    </span>
                    {pr.is_draft && (
                      <span className="text-xs px-1.5 py-0.5 rounded bg-mid-gray/30 text-mid-gray">
                        {t("devops.prs.draft")}
                      </span>
                    )}
                  </div>

                  <div className="mt-1 flex flex-wrap gap-2">
                    {/* Branch info */}
                    <span className="inline-flex items-center gap-1 text-xs px-1.5 py-0.5 rounded bg-blue-500/20 text-blue-400">
                      {pr.head_branch} → {pr.base_branch}
                    </span>
                    {pr.labels.slice(0, 2).map((label) => (
                      <span
                        key={label}
                        className="inline-flex items-center gap-1 text-xs px-1.5 py-0.5 rounded bg-mid-gray/20"
                      >
                        <Tag className="w-3 h-3" />
                        {label}
                      </span>
                    ))}
                    {pr.labels.length > 2 && (
                      <span className="text-xs text-mid-gray/70">
                        +{pr.labels.length - 2}
                      </span>
                    )}
                  </div>

                  <div className="mt-1 flex items-center gap-3 text-xs text-mid-gray/70">
                    <span className="flex items-center gap-1">
                      <User className="w-3 h-3" />
                      {pr.author}
                    </span>
                    <span className="flex items-center gap-1">
                      <Clock className="w-3 h-3" />
                      {formatDate(pr.created_at)}
                    </span>
                  </div>
                </div>

                {/* Actions */}
                <div className="flex items-center gap-1">
                  <button
                    onClick={(e) => {
                      e.stopPropagation();
                      handleExpandPr(pr.number);
                    }}
                    className="p-1.5 hover:bg-mid-gray/20 rounded transition-colors text-mid-gray"
                    title={t("devops.prs.showStatus")}
                  >
                    {expandedPr === pr.number ? (
                      <ChevronUp className="w-4 h-4" />
                    ) : (
                      <ChevronDown className="w-4 h-4" />
                    )}
                  </button>
                  <a
                    href={pr.url}
                    target="_blank"
                    rel="noopener noreferrer"
                    onClick={(e) => e.stopPropagation()}
                    className="p-1.5 hover:bg-mid-gray/20 rounded transition-colors text-mid-gray"
                    title={t("devops.prs.openInGitHub")}
                  >
                    <ExternalLink className="w-4 h-4" />
                  </a>
                </div>
              </div>

              {/* Expanded status */}
              {expandedPr === pr.number && (
                <div className="px-3 pb-3 pt-0 border-t border-mid-gray/20 mt-1">
                  {prStatuses[pr.number] ? (
                    <div className="flex flex-col gap-2 mt-2">
                      {/* Checks status */}
                      <div className="flex items-center gap-2">
                        {getCheckStatusIcon(prStatuses[pr.number].checks.state)}
                        <span className="text-xs">
                          {t("devops.prs.checks")}: {prStatuses[pr.number].checks.passing}/{prStatuses[pr.number].checks.total} {t("devops.prs.passing")}
                          {prStatuses[pr.number].checks.failing > 0 && (
                            <span className="text-red-400"> ({prStatuses[pr.number].checks.failing} {t("devops.prs.failing")})</span>
                          )}
                          {prStatuses[pr.number].checks.pending > 0 && (
                            <span className="text-yellow-400"> ({prStatuses[pr.number].checks.pending} {t("devops.prs.pending")})</span>
                          )}
                        </span>
                      </div>

                      {/* Reviews status */}
                      <div className="flex items-center gap-2">
                        {prStatuses[pr.number].reviews.approved > 0 ? (
                          <CheckCircle2 className="w-4 h-4 text-green-400" />
                        ) : prStatuses[pr.number].reviews.changes_requested > 0 ? (
                          <XCircle className="w-4 h-4 text-red-400" />
                        ) : (
                          <Clock className="w-4 h-4 text-mid-gray" />
                        )}
                        <span className="text-xs">
                          {t("devops.prs.reviews")}: {prStatuses[pr.number].reviews.approved} {t("devops.prs.approved")}
                          {prStatuses[pr.number].reviews.changes_requested > 0 && (
                            <span className="text-red-400">, {prStatuses[pr.number].reviews.changes_requested} {t("devops.prs.changesRequested")}</span>
                          )}
                        </span>
                      </div>

                      {/* Mergeable status */}
                      {pr.mergeable !== null && (
                        <div className="flex items-center gap-2">
                          {pr.mergeable ? (
                            <CheckCircle2 className="w-4 h-4 text-green-400" />
                          ) : (
                            <XCircle className="w-4 h-4 text-red-400" />
                          )}
                          <span className="text-xs">
                            {pr.mergeable ? t("devops.prs.mergeable") : t("devops.prs.notMergeable")}
                          </span>
                        </div>
                      )}
                    </div>
                  ) : (
                    <div className="flex items-center justify-center py-2">
                      <Loader2 className="w-4 h-4 animate-spin text-mid-gray" />
                    </div>
                  )}
                </div>
              )}
            </div>
          ))}
        </div>
      )}
    </div>
  );
};

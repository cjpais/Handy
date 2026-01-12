import React, { useEffect, useState, useCallback } from "react";
import { useTranslation } from "react-i18next";
import { commands, WorktreeInfo } from "@/bindings";
import {
  GitBranch,
  FolderGit2,
  Plus,
  Trash2,
  RefreshCcw,
  Loader2,
  AlertCircle,
  CheckCircle2,
  FolderOpen,
  ExternalLink,
  X,
} from "lucide-react";

interface WorktreeManagerProps {
  repoPath?: string;
  onWorktreeChange?: () => void;
}

export const WorktreeManager: React.FC<WorktreeManagerProps> = ({
  repoPath,
  onWorktreeChange,
}) => {
  const { t } = useTranslation();
  const [worktrees, setWorktrees] = useState<WorktreeInfo[]>([]);
  const [isLoading, setIsLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [removingWorktree, setRemovingWorktree] = useState<string | null>(null);
  const [showCreateDialog, setShowCreateDialog] = useState(false);
  const [currentRepoPath, setCurrentRepoPath] = useState<string | null>(null);

  // Create dialog state
  const [newWorktreeName, setNewWorktreeName] = useState("");
  const [isCreating, setIsCreating] = useState(false);
  const [createError, setCreateError] = useState<string | null>(null);

  const detectRepoPath = useCallback(async () => {
    if (repoPath) {
      setCurrentRepoPath(repoPath);
      return repoPath;
    }

    // Try to get repo root from current working directory
    try {
      const root = await commands.getGitRepoRoot(".");
      setCurrentRepoPath(root);
      return root;
    } catch {
      // Not in a git repo
      setCurrentRepoPath(null);
      return null;
    }
  }, [repoPath]);

  const loadWorktrees = useCallback(async () => {
    setIsLoading(true);
    setError(null);

    try {
      const detectedPath = await detectRepoPath();
      if (!detectedPath) {
        setWorktrees([]);
        setIsLoading(false);
        return;
      }

      const worktreeList = await commands.listGitWorktrees(detectedPath);
      setWorktrees(worktreeList);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setIsLoading(false);
    }
  }, [detectRepoPath]);

  useEffect(() => {
    loadWorktrees();
  }, [loadWorktrees]);

  const handleRemoveWorktree = async (worktreePath: string, branch?: string) => {
    if (!currentRepoPath) return;

    setRemovingWorktree(worktreePath);
    try {
      await commands.removeGitWorktree(
        currentRepoPath,
        worktreePath,
        false, // force
        branch ? true : false // delete branch if it exists
      );
      await loadWorktrees();
      onWorktreeChange?.();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setRemovingWorktree(null);
    }
  };

  const handleCreateWorktree = async () => {
    if (!currentRepoPath || !newWorktreeName.trim()) return;

    setIsCreating(true);
    setCreateError(null);

    try {
      await commands.createGitWorktree(
        currentRepoPath,
        newWorktreeName.trim(),
        null, // prefix (use default)
        null, // base_path (use default)
        null // base_branch (use default)
      );
      await loadWorktrees();
      onWorktreeChange?.();
      setShowCreateDialog(false);
      setNewWorktreeName("");
    } catch (err) {
      setCreateError(err instanceof Error ? err.message : String(err));
    } finally {
      setIsCreating(false);
    }
  };

  if (isLoading) {
    return (
      <div className="flex items-center justify-center p-8">
        <Loader2 className="w-6 h-6 animate-spin text-logo-primary" />
      </div>
    );
  }

  if (!currentRepoPath) {
    return (
      <div className="flex flex-col items-center justify-center p-8 text-center">
        <FolderGit2 className="w-12 h-12 text-mid-gray/50 mb-3" />
        <p className="text-sm text-mid-gray">
          {t("devops.worktrees.notInRepo")}
        </p>
        <p className="text-xs text-mid-gray/70 mt-1">
          {t("devops.worktrees.notInRepoHint")}
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
          onClick={loadWorktrees}
          className="ml-auto p-1 hover:bg-mid-gray/20 rounded"
        >
          <RefreshCcw className="w-4 h-4" />
        </button>
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-3">
      {/* Header with actions */}
      <div className="flex items-center justify-between">
        <span className="text-sm text-mid-gray">
          {t("devops.worktrees.count", { count: worktrees.length })}
        </span>
        <div className="flex items-center gap-2">
          <button
            onClick={() => setShowCreateDialog(true)}
            className="flex items-center gap-1 px-2 py-1 text-sm rounded bg-logo-primary/20 hover:bg-logo-primary/30 transition-colors text-logo-primary"
          >
            <Plus className="w-4 h-4" />
            {t("devops.worktrees.create")}
          </button>
          <button
            onClick={loadWorktrees}
            disabled={isLoading}
            className="p-1 hover:bg-mid-gray/20 rounded transition-colors"
            title={t("devops.refresh")}
          >
            <RefreshCcw className={`w-4 h-4 ${isLoading ? "animate-spin" : ""}`} />
          </button>
        </div>
      </div>

      {/* Create Dialog */}
      {showCreateDialog && (
        <div className="p-3 rounded-lg bg-mid-gray/10 border border-mid-gray/20">
          <div className="flex items-center justify-between mb-3">
            <span className="text-sm font-medium">
              {t("devops.worktrees.createTitle")}
            </span>
            <button
              onClick={() => {
                setShowCreateDialog(false);
                setNewWorktreeName("");
                setCreateError(null);
              }}
              className="p-1 hover:bg-mid-gray/20 rounded"
            >
              <X className="w-4 h-4" />
            </button>
          </div>

          <div className="flex flex-col gap-2">
            <input
              type="text"
              value={newWorktreeName}
              onChange={(e) => setNewWorktreeName(e.target.value)}
              placeholder={t("devops.worktrees.namePlaceholder")}
              className="px-3 py-2 rounded bg-mid-gray/10 border border-mid-gray/20 text-sm focus:outline-none focus:border-logo-primary"
              onKeyDown={(e) => {
                if (e.key === "Enter" && newWorktreeName.trim()) {
                  handleCreateWorktree();
                }
              }}
            />

            {createError && (
              <div className="flex items-center gap-2 text-red-400 text-xs">
                <AlertCircle className="w-3 h-3" />
                {createError}
              </div>
            )}

            <button
              onClick={handleCreateWorktree}
              disabled={!newWorktreeName.trim() || isCreating}
              className="flex items-center justify-center gap-2 px-3 py-2 rounded bg-logo-primary hover:bg-logo-primary/90 disabled:opacity-50 disabled:cursor-not-allowed transition-colors text-sm"
            >
              {isCreating ? (
                <Loader2 className="w-4 h-4 animate-spin" />
              ) : (
                <Plus className="w-4 h-4" />
              )}
              {t("devops.worktrees.createButton")}
            </button>
          </div>
        </div>
      )}

      {/* Worktree list */}
      {worktrees.length === 0 ? (
        <div className="flex flex-col items-center justify-center p-8 text-center">
          <FolderGit2 className="w-12 h-12 text-mid-gray/50 mb-3" />
          <p className="text-sm text-mid-gray">
            {t("devops.worktrees.noWorktrees")}
          </p>
          <p className="text-xs text-mid-gray/70 mt-1">
            {t("devops.worktrees.noWorktreesHint")}
          </p>
        </div>
      ) : (
        <div className="flex flex-col gap-2">
          {worktrees.map((worktree) => (
            <div
              key={worktree.path}
              className="flex items-start gap-3 p-3 rounded-lg bg-mid-gray/10 hover:bg-mid-gray/15 transition-colors"
            >
              {/* Icon */}
              <div className="mt-1">
                {worktree.is_main ? (
                  <FolderGit2 className="w-4 h-4 text-logo-primary" />
                ) : (
                  <FolderOpen className="w-4 h-4 text-blue-400" />
                )}
              </div>

              {/* Content */}
              <div className="flex-1 min-w-0">
                <div className="flex items-center gap-2">
                  <code className="font-medium text-sm truncate">
                    {worktree.path.split("/").pop() || worktree.path}
                  </code>
                  {worktree.is_main && (
                    <span className="text-xs px-1.5 py-0.5 rounded bg-logo-primary/20 text-logo-primary">
                      {t("devops.worktrees.main")}
                    </span>
                  )}
                  {worktree.is_locked && (
                    <span className="text-xs px-1.5 py-0.5 rounded bg-yellow-500/20 text-yellow-400">
                      {t("devops.worktrees.locked")}
                    </span>
                  )}
                  {worktree.is_prunable && (
                    <span className="text-xs px-1.5 py-0.5 rounded bg-red-500/20 text-red-400">
                      {t("devops.worktrees.prunable")}
                    </span>
                  )}
                </div>

                <div className="mt-1 text-xs text-mid-gray space-y-0.5">
                  {worktree.branch && (
                    <div className="flex items-center gap-1">
                      <GitBranch className="w-3 h-3" />
                      <span>{worktree.branch}</span>
                    </div>
                  )}
                  <div className="flex items-center gap-1 truncate">
                    <FolderOpen className="w-3 h-3 shrink-0" />
                    <span className="truncate" title={worktree.path}>
                      {worktree.path}
                    </span>
                  </div>
                </div>
              </div>

              {/* Actions */}
              {!worktree.is_main && (
                <div className="flex items-center gap-1">
                  <button
                    onClick={() => handleRemoveWorktree(worktree.path, worktree.branch ?? undefined)}
                    disabled={removingWorktree === worktree.path}
                    className="p-1.5 hover:bg-red-500/20 rounded transition-colors text-red-400"
                    title={t("devops.worktrees.remove")}
                  >
                    {removingWorktree === worktree.path ? (
                      <Loader2 className="w-4 h-4 animate-spin" />
                    ) : (
                      <Trash2 className="w-4 h-4" />
                    )}
                  </button>
                </div>
              )}
            </div>
          ))}
        </div>
      )}
    </div>
  );
};

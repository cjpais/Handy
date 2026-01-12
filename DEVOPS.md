# DevOps Tab - Multi-Agent Coding Assistant

## Overview

The DevOps tab provides an interface for managing multiple coding agents (like Claude Code) to help with development workflows. It integrates with terminal multiplexing (tmux) and GitHub CLI (gh) to enable parallel agent execution and seamless GitHub operations.

## Prerequisites

The DevOps tab requires the following CLI tools to be installed:

| Tool | Purpose | Installation |
|------|---------|--------------|
| `gh` | GitHub CLI for PR management, issues, repo operations | `brew install gh` |
| `tmux` | Terminal multiplexer for managing agent sessions | `brew install tmux` |

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                        Handy DevOps Tab                         │
├─────────────────────────────────────────────────────────────────┤
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐              │
│  │ Agent Pool  │  │ Task Queue  │  │ GitHub Ops  │              │
│  └──────┬──────┘  └──────┬──────┘  └──────┬──────┘              │
│         │                │                │                      │
│         └────────────────┼────────────────┘                      │
│                          │                                       │
│                    ┌─────┴─────┐                                 │
│                    │  Tauri    │                                 │
│                    │  Backend  │                                 │
│                    └─────┬─────┘                                 │
│                          │                                       │
├──────────────────────────┼──────────────────────────────────────┤
│                          │                                       │
│         ┌────────────────┼────────────────┐                      │
│         │                │                │                      │
│    ┌────┴────┐     ┌─────┴─────┐    ┌─────┴─────┐               │
│    │  tmux   │     │    gh     │    │  Agents   │               │
│    │ sessions│     │   CLI     │    │ (claude)  │               │
│    └─────────┘     └───────────┘    └───────────┘               │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

## Implementation Plan

### Phase 1: Foundation (This Patch)

#### 1.1 Backend - Dependency Detection
- [ ] Create `src-tauri/src/devops/mod.rs` module
- [ ] Add `check_dependencies()` command to detect `gh` and `tmux`
- [ ] Return structured status for each dependency (installed, version, path)

#### 1.2 Frontend - DevOps Tab Shell
- [ ] Create `src/components/settings/devops/DevOpsSettings.tsx`
- [ ] Add DevOps tab to settings navigation
- [ ] Display dependency status with install instructions if missing
- [ ] Add i18n translations for DevOps UI

### Phase 2: tmux Integration

#### 2.1 Session Management
- [ ] `list_tmux_sessions()` - List all tmux sessions
- [ ] `create_tmux_session(name)` - Create named session
- [ ] `kill_tmux_session(name)` - Terminate session
- [ ] `attach_tmux_session(name)` - Get session output/status

#### 2.2 Agent Spawning
- [ ] `spawn_agent(session_name, agent_type, task)` - Launch agent in tmux
- [ ] Support for different agent types (claude, aider, etc.)
- [ ] Working directory configuration per agent
- [ ] Environment variable passthrough

### Phase 3: Worktree Management

The worktree system enables isolated development environments for each agent, preventing conflicts when multiple agents work in parallel.

#### 3.1 Worktree Lifecycle
```
┌──────────────┐     ┌──────────────┐     ┌──────────────┐     ┌──────────────┐
│   Create     │────▶│   Spawn      │────▶│   Work       │────▶│   Merge &    │
│   Worktree   │     │   Agent      │     │   Complete   │     │   Cleanup    │
└──────────────┘     └──────────────┘     └──────────────┘     └──────────────┘
      │                    │                    │                    │
      ▼                    ▼                    ▼                    ▼
 {project}-{name}    tmux session         Commits ready      git merge +
 e.g. Handy-fix-1    in worktree          for review         worktree remove
```

#### 3.2 Worktree Commands
- [ ] `list_worktrees()` - List all git worktrees with status
- [ ] `create_worktree(name)` - Create worktree with collision checks:
  - Validates not inside existing worktree
  - Checks for existing directory with same name
  - Checks for existing branch with same name
  - Uses configurable prefix (default: `{project}-`)
  - Creates new branch and worktree atomically
- [ ] `remove_worktree(path)` - Clean up worktree and optionally delete branch
- [ ] `merge_worktree(path, target)` - Merge worktree branch into target, then cleanup

#### 3.3 Worktree Configuration
```rust
#[derive(Serialize, Deserialize, Type)]
struct WorktreeConfig {
    /// Prefix for worktree directories (e.g., "Handy-" -> "Handy-feature-1")
    prefix: String,
    /// Base directory for worktrees (default: parent of repo)
    base_path: Option<String>,
    /// Auto-delete branch after merge
    delete_branch_on_merge: bool,
}
```

### Phase 4: GitHub Integration

#### 4.1 Authentication & Status
- [ ] `gh_auth_status()` - Check GitHub authentication
- [ ] `gh_auth_login()` - Trigger login flow if needed

#### 4.2 Repository Operations
- [ ] `gh_repo_info()` - Get current repo info
- [ ] `gh_list_prs()` - List open PRs
- [ ] `gh_list_issues()` - List open issues
- [ ] `gh_create_pr(title, body, base)` - Create PR from current branch

### Phase 5: Multi-Agent Orchestration

#### 5.1 Task Distribution
- [ ] Task queue system for distributing work to agents
- [ ] Agent status monitoring (idle, working, blocked)
- [ ] Real-time output streaming from agent sessions

#### 5.2 Coordination
- [ ] Branch/worktree assignment per agent
- [ ] Conflict detection when agents work on same files
- [ ] Merge coordination between agent outputs

#### 5.3 Templates
- [ ] Pre-defined task templates (bug fix, feature, refactor)
- [ ] Custom prompt templates for agents
- [ ] Project-specific agent configurations

## File Structure

```
src-tauri/src/
├── devops/
│   ├── mod.rs           # Module exports
│   ├── dependencies.rs  # gh/tmux detection
│   ├── tmux.rs          # tmux session management
│   ├── github.rs        # gh CLI wrapper
│   ├── worktree.rs      # Git worktree management
│   └── agents.rs        # Agent spawning/management

src/components/settings/devops/
├── DevOpsSettings.tsx   # Main DevOps tab component
├── DependencyStatus.tsx # Shows gh/tmux status
├── SessionManager.tsx   # tmux session list/controls
├── AgentPanel.tsx       # Individual agent view
├── TaskQueue.tsx        # Pending tasks display
├── GitHubPanel.tsx      # PR/Issue integration
└── WorktreeManager.tsx  # Worktree list/create/merge UI

src/i18n/locales/en/
└── translation.json     # Add devops.* keys
```

## Tauri Commands

### Phase 1 Commands

```rust
#[tauri::command]
async fn check_devops_dependencies() -> Result<DevOpsDependencies, String>

#[derive(Serialize, Deserialize, Type)]
struct DevOpsDependencies {
    gh: DependencyStatus,
    tmux: DependencyStatus,
}

#[derive(Serialize, Deserialize, Type)]
struct DependencyStatus {
    installed: bool,
    version: Option<String>,
    path: Option<String>,
}
```

## UI Mockup

```
┌─────────────────────────────────────────────────────────────┐
│ DevOps                                                      │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  Dependencies                                               │
│  ┌─────────────────────────────────────────────────────┐   │
│  │ ✓ gh      v2.40.0   /opt/homebrew/bin/gh            │   │
│  │ ✓ tmux    v3.4      /opt/homebrew/bin/tmux          │   │
│  └─────────────────────────────────────────────────────┘   │
│                                                             │
│  Active Sessions                              [+ New Agent] │
│  ┌─────────────────────────────────────────────────────┐   │
│  │ agent-1  │ claude │ feature-auth │ Working...       │   │
│  │ agent-2  │ claude │ fix-bug-123  │ Idle             │   │
│  └─────────────────────────────────────────────────────┘   │
│                                                             │
│  Task Queue                                                 │
│  ┌─────────────────────────────────────────────────────┐   │
│  │ 1. Implement logout button         [Assign Agent ▼] │   │
│  │ 2. Fix memory leak in dashboard    [Assign Agent ▼] │   │
│  └─────────────────────────────────────────────────────┘   │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

## Example Workflow: Multi-Agent Feature Development

```
User: "I need to implement user authentication and a dashboard"

1. DevOps creates two worktrees:
   - Handy-auth-feature (for authentication)
   - Handy-dashboard-feature (for dashboard)

2. DevOps spawns agents in parallel:
   ┌─────────────────────────────────────────────────────────────┐
   │ tmux: agent-auth                 │ tmux: agent-dashboard    │
   │ cwd: ../Handy-auth-feature       │ cwd: ../Handy-dashboard  │
   │ task: "Implement user auth..."   │ task: "Build dashboard..." │
   │ status: Working                  │ status: Working          │
   └─────────────────────────────────────────────────────────────┘

3. Agents work independently (no conflicts - separate worktrees)

4. Agent completes → DevOps shows notification:
   "agent-auth completed: 3 commits ready for review"
   [View Diff] [Merge to main] [Create PR]

5. User clicks "Merge to main":
   - git merge auth-feature (from main repo)
   - git worktree remove ../Handy-auth-feature
   - git branch -d auth-feature
   - Notification: "auth-feature merged and cleaned up"

6. Repeat for dashboard when ready
```

## Security Considerations

- All CLI commands executed via Tauri's shell API with proper escaping
- No arbitrary command execution - only predefined operations
- GitHub tokens managed by `gh` CLI, not stored by Handy
- tmux sessions isolated per-project

## Future Enhancements

- **Agent Memory**: Share context between agents via memory system
- **Voice Commands**: "Spawn an agent to fix issue 123"
- **Auto-merge**: Automatically merge agent PRs after CI passes
- **Cost Tracking**: Monitor API usage across agents
- **Diff Review**: Built-in diff viewer for agent changes

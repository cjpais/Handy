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

tmux sessions persist independently of Handy, enabling recovery after hot reloads, crashes, or app restarts.

#### 2.1 Session Persistence Architecture
```
┌─────────────────────────────────────────────────────────────────┐
│                         tmux server                              │
│                    (runs independently)                          │
├─────────────────────────────────────────────────────────────────┤
│  handy-agent-42     │ handy-agent-43     │ handy-agent-15       │
│  ├── issue: #42     │ ├── issue: #43     │ ├── issue: #15       │
│  ├── repo: frontend │ ├── repo: frontend │ ├── repo: backend    │
│  └── status: active │ └── status: active │ └── status: active   │
└─────────────────────────────────────────────────────────────────┘
                              ▲
                              │ survives restart
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│  Handy App                                                       │
│  ┌─────────────────────────────────────────────────────────────┐│
│  │ On startup: recover_agent_sessions()                        ││
│  │   1. List tmux sessions matching "handy-agent-*"            ││
│  │   2. Parse session metadata from env vars                   ││
│  │   3. Rebuild agent state from session info                  ││
│  │   4. Resume monitoring output                               ││
│  └─────────────────────────────────────────────────────────────┘│
└─────────────────────────────────────────────────────────────────┘
```

#### 2.2 Session Naming Convention
```
handy-agent-{issue_number}[-{suffix}]

Examples:
  handy-agent-42           # Working on issue #42
  handy-agent-42-retry     # Retry attempt for #42
  handy-agent-manual-1     # Manual session without issue
```

#### 2.3 Dual-Layer Metadata (tmux + GitHub Issue)

Metadata is stored in two places for redundancy:

```
┌─────────────────────────────────────────────────────────────────┐
│                    Metadata Recovery Layers                      │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  Layer 1: tmux environment (fast, local)                        │
│  ┌───────────────────────────────────────────────────────────┐  │
│  │ HANDY_ISSUE_REF="org/repo#42"                             │  │
│  │ HANDY_WORKTREE="/path/to/worktree"                        │  │
│  │ HANDY_AGENT_TYPE="claude"                                 │  │
│  │ HANDY_STARTED_AT="2024-01-15T10:30:00Z"                   │  │
│  └───────────────────────────────────────────────────────────┘  │
│                         ▼ fallback                               │
│  Layer 2: GitHub issue comment (persistent, cross-machine)      │
│  ┌───────────────────────────────────────────────────────────┐  │
│  │ <!-- HANDY_AGENT_METADATA                                 │  │
│  │ {                                                         │  │
│  │   "session": "handy-agent-42",                            │  │
│  │   "worktree": "/path/to/worktree",                        │  │
│  │   "agent_type": "claude",                                 │  │
│  │   "machine_id": "macbook-pro-1",                          │  │
│  │   "started_at": "2024-01-15T10:30:00Z",                   │  │
│  │   "status": "working"                                     │  │
│  │ }                                                         │  │
│  │ -->                                                       │  │
│  │ 🤖 **Agent Assigned**                                     │  │
│  │ - Session: `handy-agent-42`                               │  │
│  │ - Type: claude                                            │  │
│  │ - Started: Jan 15, 2024 10:30 AM                          │  │
│  └───────────────────────────────────────────────────────────┘  │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

**tmux environment (Layer 1):**
```bash
# Set when spawning agent
tmux set-environment -t handy-agent-42 HANDY_ISSUE_REF "org/repo#42"
tmux set-environment -t handy-agent-42 HANDY_REPO "org/repo"
tmux set-environment -t handy-agent-42 HANDY_WORKTREE "/path/to/worktree"
tmux set-environment -t handy-agent-42 HANDY_AGENT_TYPE "claude"
tmux set-environment -t handy-agent-42 HANDY_MACHINE_ID "$(hostname)"
tmux set-environment -t handy-agent-42 HANDY_STARTED_AT "2024-01-15T10:30:00Z"

# Read during recovery
tmux show-environment -t handy-agent-42
```

**GitHub issue comment (Layer 2):**
```bash
# Posted when agent starts (hidden metadata + visible status)
gh issue comment 42 --repo org/repo --body "$(cat <<'EOF'
<!-- HANDY_AGENT_METADATA
{"session":"handy-agent-42","worktree":"/path/to/worktree","agent_type":"claude","machine_id":"macbook-pro-1","started_at":"2024-01-15T10:30:00Z","status":"working"}
-->
🤖 **Agent Assigned**
- Session: `handy-agent-42`
- Type: claude
- Machine: macbook-pro-1
- Started: Jan 15, 2024 10:30 AM
EOF
)"

# Updated periodically with progress
gh issue comment 42 --repo org/repo --body "📊 **Progress Update**
- Commits: 3
- Files changed: 5
- Last activity: 2 minutes ago"
```

#### 2.4 Recovery Priority

```
On Startup:
1. Check tmux for handy-agent-* sessions (fast, local)
   ├── Found? → Read HANDY_* env vars → Resume monitoring
   └── Not found? → Check GitHub issues

2. Query GitHub for issues with agent-assigned label
   ├── Parse HANDY_AGENT_METADATA from comments
   ├── Filter by machine_id (only recover our sessions)
   └── Check if worktree still exists
       ├── Exists + no tmux? → Session crashed, offer restart
       └── Missing? → Agent completed or was cleaned up

3. Reconcile state:
   - tmux alive + issue open → Working normally
   - tmux dead + issue open → Crashed, offer restart
   - tmux alive + issue closed → Orphan session, offer cleanup
   - tmux dead + issue closed → Completed, nothing to do
```

#### 2.5 Session Commands
- [ ] `list_tmux_sessions()` - List all tmux sessions (filter by handy-agent-* prefix)
- [ ] `create_tmux_session(name)` - Create named session with metadata
- [ ] `kill_tmux_session(name)` - Terminate session
- [ ] `get_session_output(name, lines?)` - Get recent output from session
- [ ] `recover_agent_sessions()` - Rebuild state from tmux + GitHub fallback
- [ ] `get_session_metadata(name)` - Read HANDY_* env vars from session
- [ ] `sync_issue_metadata(issue_ref, metadata)` - Update hidden metadata in issue comment
- [ ] `parse_issue_metadata(issue_ref)` - Extract HANDY_AGENT_METADATA from comments

#### 2.6 Recovery Flow
```rust
#[derive(Serialize, Deserialize, Type)]
struct AgentMetadata {
    session: String,
    issue_ref: String,
    worktree: String,
    agent_type: String,
    machine_id: String,
    started_at: String,
    status: AgentStatus,  // working, completed, crashed, orphaned
}

#[derive(Serialize, Deserialize, Type)]
enum RecoverySource {
    Tmux,           // Found in tmux, normal operation
    GitHubIssue,    // Recovered from issue comment
    Both,           // Confirmed by both sources
}

#[derive(Serialize, Deserialize, Type)]
struct RecoveredSession {
    metadata: AgentMetadata,
    source: RecoverySource,
    tmux_alive: bool,
    worktree_exists: bool,
    issue_open: bool,
    recommended_action: RecoveryAction,
}

#[derive(Serialize, Deserialize, Type)]
enum RecoveryAction {
    Resume,         // tmux alive, continue monitoring
    Restart,        // tmux dead but work incomplete, offer restart
    Cleanup,        // orphan session, offer to kill/remove
    None,           // completed normally, nothing to do
}
```

#### 2.7 Agent Spawning
- [ ] `spawn_agent(session_name, agent_type, task)` - Launch agent in tmux
- [ ] Support for different agent types (claude, aider, etc.)
- [ ] Working directory configuration per agent
- [ ] Environment variable passthrough
- [ ] Store metadata for recovery

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

### Phase 4: GitHub Issue-Driven Tasks

Tasks are backed by GitHub issues, providing traceability, cross-repo coordination, and a single source of truth.

#### 4.1 Issue Hub Architecture
```
┌─────────────────────────────────────────────────────────────────┐
│                     Parent Issue Repo                            │
│                  (e.g., org/project-tasks)                       │
├─────────────────────────────────────────────────────────────────┤
│  #42 [Epic] User Authentication                                  │
│   ├── #43 org/frontend: Login UI         → agent-1 (working)    │
│   ├── #44 org/backend: Auth API          → agent-2 (working)    │
│   └── #45 org/shared: Auth types         → agent-3 (idle)       │
│                                                                  │
│  #50 [Epic] Dashboard Redesign                                   │
│   ├── #51 org/frontend: New layout       → unassigned           │
│   └── #52 org/analytics: Metrics API     → unassigned           │
└─────────────────────────────────────────────────────────────────┘
```

#### 4.2 Issue Configuration
```rust
#[derive(Serialize, Deserialize, Type)]
struct IssueHubConfig {
    /// Parent repo for coordinating issues (e.g., "org/project-tasks")
    hub_repo: Option<String>,
    /// Repos this DevOps instance manages
    managed_repos: Vec<String>,
    /// Label to identify agent-workable issues
    agent_label: String,  // default: "agent-ready"
    /// Auto-create issues when spawning agents
    auto_create_issues: bool,
}

#[derive(Serialize, Deserialize, Type)]
struct TaskIssue {
    /// Full issue reference (e.g., "org/repo#123")
    issue_ref: String,
    /// Issue title
    title: String,
    /// Target repo for the work (may differ from issue repo)
    target_repo: String,
    /// Assigned agent session (if any)
    agent_session: Option<String>,
    /// Parent epic issue (if any)
    parent_issue: Option<String>,
    /// Issue state
    state: IssueState,
}
```

#### 4.3 Issue Commands
- [ ] `configure_issue_hub(config)` - Set up parent repo for cross-repo coordination
- [ ] `list_agent_issues(repo?)` - List issues with agent-ready label
- [ ] `create_task_issue(repo, title, body, parent?)` - Create issue, optionally link to epic
- [ ] `assign_issue_to_agent(issue_ref, agent_session)` - Link issue to running agent
- [ ] `close_issue_with_pr(issue_ref, pr_url)` - Close issue when PR merges
- [ ] `sync_issue_status(issue_ref)` - Update issue comments with agent progress

#### 4.4 Cross-Repo Workflow
```
1. User creates epic in hub repo: org/tasks#42 "User Authentication"

2. DevOps breaks down into sub-issues across repos:
   - org/frontend#101 "Login UI component"        (links to #42)
   - org/backend#55 "Auth API endpoints"          (links to #42)
   - org/shared#12 "Shared auth types"            (links to #42)

3. Each sub-issue gets:
   - Its own worktree in the target repo
   - Its own agent session
   - Progress comments synced back to the issue

4. When agent completes:
   - PR created in target repo, references issue
   - Issue closed automatically when PR merges
   - Parent epic updated with completion status
```

### Phase 5: GitHub Integration

#### 5.1 Authentication & Status
- [ ] `gh_auth_status()` - Check GitHub authentication
- [ ] `gh_auth_login()` - Trigger login flow if needed

#### 5.2 Repository Operations
- [ ] `gh_repo_info()` - Get current repo info
- [ ] `gh_list_prs()` - List open PRs
- [ ] `gh_list_issues()` - List open issues
- [ ] `gh_create_pr(title, body, base)` - Create PR from current branch

### Phase 6: Multi-Agent Orchestration

#### 6.1 Task Distribution
- [ ] Issue queue populated from GitHub (agent-ready label)
- [ ] Agent status monitoring (idle, working, blocked)
- [ ] Real-time output streaming from agent sessions

#### 6.2 Coordination
- [ ] Branch/worktree assignment per agent
- [ ] Conflict detection when agents work on same files
- [ ] Merge coordination between agent outputs

#### 6.3 Templates
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
│   ├── github.rs        # gh CLI wrapper (auth, PRs)
│   ├── issues.rs        # Issue hub & cross-repo task management
│   ├── worktree.rs      # Git worktree management
│   └── agents.rs        # Agent spawning/management

src/components/settings/devops/
├── DevOpsSettings.tsx   # Main DevOps tab component
├── DependencyStatus.tsx # Shows gh/tmux status
├── SessionManager.tsx   # tmux session list/controls
├── AgentPanel.tsx       # Individual agent view
├── IssueQueue.tsx       # GitHub issues as task queue
├── IssueHubConfig.tsx   # Configure parent repo & managed repos
├── GitHubPanel.tsx      # PR integration
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

## Example Workflow: Issue-Driven Multi-Agent Development

```
User: "I need to implement user authentication and a dashboard"

1. DevOps creates epic issue in hub repo:
   → myorg/tasks#100 "[Epic] User Auth & Dashboard"

2. DevOps creates linked sub-issues:
   → myorg/frontend#42 "Login UI component"      (parent: tasks#100)
   → myorg/frontend#43 "Dashboard layout"        (parent: tasks#100)
   → myorg/backend#15 "Auth API endpoints"       (parent: tasks#100)

3. For each issue, DevOps:
   a. Clones/opens the target repo
   b. Creates worktree: frontend-issue-42
   c. Spawns agent in tmux with issue context
   d. Updates issue: "🤖 Agent assigned, working..."

   ┌─────────────────────────────────────────────────────────────┐
   │ tmux: agent-42                   │ tmux: agent-43           │
   │ repo: myorg/frontend             │ repo: myorg/frontend     │
   │ cwd: ../frontend-issue-42        │ cwd: ../frontend-issue-43│
   │ issue: #42 Login UI              │ issue: #43 Dashboard     │
   │ status: Working                  │ status: Working          │
   └─────────────────────────────────────────────────────────────┘

   ┌─────────────────────────────────────────────────────────────┐
   │ tmux: agent-15                                              │
   │ repo: myorg/backend                                         │
   │ cwd: ../backend-issue-15                                    │
   │ issue: #15 Auth API                                         │
   │ status: Working                                             │
   └─────────────────────────────────────────────────────────────┘

4. Agents work independently (no conflicts - separate repos/worktrees)

5. Agent completes → DevOps:
   a. Comments on issue: "✅ Implementation complete, 3 commits"
   b. Creates PR: "Closes #42" with agent's changes
   c. Updates epic: "1/3 sub-tasks complete"
   d. Shows notification: [View PR] [View Issue]

6. When PR merges:
   - Issue #42 auto-closes (via "Closes #42" in PR)
   - Worktree cleaned up
   - Epic #100 progress updated

7. Epic shows full status:
   myorg/tasks#100:
   ✅ frontend#42 Login UI - merged
   🔄 frontend#43 Dashboard - PR open
   🤖 backend#15 Auth API - agent working
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

# Blockers

Hard blockers that could not be worked around. Anything not listed here was
solved or has a documented workaround in DECISIONS.md.

| #    | Blocker                                                                                                                                                                                                                                                                                                                                                                                                                                                         | Impact                                                                                                                                                                                                                                                  | Status                                                           |
| ---- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------- |
| B-01 | No macOS machine in the sandbox, so the app cannot be launched and the native glass effect, hotkeys, paste and microphone cannot be exercised.                                                                                                                                                                                                                                                                                                                  | Everything native is verified only by compiling in CI. Manual checklist for the owner is in `FINAL_REPORT.md`.                                                                                                                                          | Open by nature of the environment                                |
| B-02 | HuggingFace is blocked by the sandbox proxy (HTTP 403 on CONNECT).                                                                                                                                                                                                                                                                                                                                                                                              | No model download and no real transcription locally; `p95` latency on the M2 is not measured here.                                                                                                                                                      | Open, CI runners can reach it but nothing downloads models there |
| B-03 | **No write access to GitHub from the session.** `git push` returns 403 ("Claude doesn't have GitHub access to Dudu2I3/voxa for your organization", i.e. the Claude GitHub App is not installed on the repo); the GitHub MCP `create_branch` returns 403 "Resource not accessible by integration"; `gh api -X POST .../git/refs` returns 403 "Write access to this GitHub API path is not permitted through this proxy". Reading (files, workflows, runs) works. | The branch cannot be pushed, so the `voxa-macos` workflow cannot run and no CI logs or artifact can be read, and the PR cannot be opened. Acceptance items 1 (green CI, `Voxa-macos-arm64.zip`) and 7 (PR) are therefore not delivered by this session. | Open. See "How to unblock" below.                                |

## How to unblock B-03

All work is committed on the local branch `claude/magical-ritchie-lg9k96` and
exported as a git bundle that was sent to the owner at the end of the session.

1. Install the Claude GitHub App on `Dudu2I3/voxa`
   (https://github.com/apps/claude/installations/select_target), or push by hand:
   `git fetch voxa.bundle claude/magical-ritchie-lg9k96:claude/magical-ritchie-lg9k96 && git push -u origin claude/magical-ritchie-lg9k96`.
2. The push triggers `voxa-macos` automatically (branch filter `claude/**`).
   Download the `Voxa-macos-arm64` artifact from the run.
3. Open the PR `claude/magical-ritchie-lg9k96 -> main` with `docs/PR_DESCRIPTION.md`
   as the body.

Because CI could not run, the macOS-only code paths (`cfg(target_os = "macos")`)
were kept minimal and were reviewed by hand; everything else was compiled and
tested on Linux. `docs/BUILD_LOCAL.md` has the single command for a local Mac
build.

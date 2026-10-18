# Blockers

Hard blockers that could not be worked around. Anything not listed here was
solved or has a documented workaround in DECISIONS.md.

| #    | Blocker                                                                                                                                                                                                                                                                                                                                                                                                                                                         | Impact                                                                                                                                                                                                                                                  | Status                                                           |
| ---- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------- |
| B-01 | No macOS machine in the sandbox, so the app cannot be launched and the native glass effect, hotkeys, paste and microphone cannot be exercised.                                                                                                                                                                                                                                                                                                                  | Everything native is verified only by compiling in CI. Manual checklist for the owner is in `FINAL_REPORT.md`.                                                                                                                                          | Open by nature of the environment                                |
| B-02 | HuggingFace is blocked by the sandbox proxy (HTTP 403 on CONNECT).                                                                                                                                                                                                                                                                                                                                                                                              | No model download and no real transcription locally; `p95` latency on the M2 is not measured here.                                                                                                                                                      | Open, CI runners can reach it but nothing downloads models there |
| B-03 | **No write access to GitHub from the session.** `git push` returns 403 ("Claude doesn't have GitHub access to Dudu2I3/voxa for your organization", i.e. the Claude GitHub App is not installed on the repo); the GitHub MCP `create_branch` returns 403 "Resource not accessible by integration"; `gh api -X POST .../git/refs` returns 403 "Write access to this GitHub API path is not permitted through this proxy". Reading (files, workflows, runs) works. | The branch cannot be pushed, so the `voxa-macos` workflow cannot run and no CI logs or artifact can be read, and the PR cannot be opened. Acceptance items 1 (green CI, `Voxa-macos-arm64.zip`) and 7 (PR) are therefore not delivered by this session. | Resolved: GitHub connected, branch pushed, CI green.             |

## B-03 resolution

The owner connected GitHub to the session. `git push` of
`claude/magical-ritchie-lg9k96` succeeded and triggered `voxa-macos`
(run 39063831146): `Lint and tests` and `Build macOS arm64` both passed and the
artifact `Voxa-macos-arm64` was uploaded. The PR into `main` was opened and then
closed on the owner's request (not needed).

The macOS-only code paths (`cfg(target_os = "macos")`) compile in CI but were not
run on a real Mac; see B-01 and the manual checklist in `FINAL_REPORT.md`.
`docs/BUILD_LOCAL.md` has the single command for a local Mac build.

## B-04 Secrets cannot be set from the session

`gh secret list` answers `HTTP 403: Access to this GitHub Actions path is not permitted through this proxy`, so the session cannot create the updater key, the signing certificate or the repository secrets (PLAN-2, A.5). Not worked around.

Effect: `voxa-release` runs green without secrets (ad-hoc signature, no release published, a warning in the log), and `tauri.conf.json` keeps `PLACEHOLDER_SET_BY_RELEASE_WORKFLOW` as the updater public key. The app then reports "updates are not set up yet".

How to unblock: follow [SETUP_SECRETS.md](SETUP_SECRETS.md) (about 10 minutes, one time).

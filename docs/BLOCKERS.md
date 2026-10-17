# Blockers

Hard blockers that could not be worked around. Anything not listed here was
solved or has a documented workaround in DECISIONS.md.

| #    | Blocker                                                                                                                                        | Impact                                                                                                         | Status                                                           |
| ---- | ---------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------- |
| B-01 | No macOS machine in the sandbox, so the app cannot be launched and the native glass effect, hotkeys, paste and microphone cannot be exercised. | Everything native is verified only by compiling in CI. Manual checklist for the owner is in `FINAL_REPORT.md`. | Open by nature of the environment                                |
| B-02 | HuggingFace is blocked by the sandbox proxy (HTTP 403 on CONNECT).                                                                             | No model download and no real transcription locally; `p95` latency on the M2 is not measured here.             | Open, CI runners can reach it but nothing downloads models there |

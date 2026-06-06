# Implementation Plan - Gmail & Google Tasks Integration

## Objectives

- Implement Google OAuth 2.0 flow using a local TCP loopback server in Rust backend.
- Save Google refresh token in `AppSettings` using `tauri-plugin-store`.
- Automatically refresh access tokens.
- Add backend client code for Gmail and Google Tasks APIs using `reqwest`.
- Add `default_meeting_notes_with_actions` prompt in settings to instruct the LLM to output a JSON object with `summary` and `action_items` fields.
- Integrate "Google Services" connection section in the settings tab.
- Add "Send via Google" button and recipient email input dialog in meeting summary UI.
- Provide clear success/error toast notifications and loading states.

## Milestones

1. **E2E Test Suite (Milestone 1)**: Build test infra, create Tier 1-4 playwright tests.
2. **Backend OAuth (Milestone 2)**: Local TCP loopback listener, OAuth token request/refresh logic, store token in settings.
3. **Backend Clients & Commands (Milestone 3)**: Implement Gmail client (RFC 2822 formatting + send) and Google Tasks client (adding to default list), Tauri commands.
4. **Frontend UI & Actions (Milestone 4)**: Google Services settings connection section, Send via Google button in meeting summary, recipient email dialog, loading & toasts.
5. **E2E Verification Pass (Milestone 5)**: Execute E2E test runner, debug and fix issues, pass all tiers.
6. **Adversarial Hardening (Milestone 6)**: White-box analysis, generate Tier 5 test cases, verify no gaps.

## Verification Strategy

- Run playwright tests: `bun run test:playwright`
- Linting and Formatting: `bun run lint`, `bun run format`
- Forensic Auditor integrity checks.

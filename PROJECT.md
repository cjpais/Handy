# Project: Gmail & Google Tasks Integration

## Architecture

- **Rust Backend**:
  - `src-tauri/src/settings.rs`: Add `google_refresh_token` to `AppSettings`, register `default_meeting_notes_with_actions` prompt with JSON format instruction.
  - `src-tauri/src/commands/google_auth.rs`: Define Tauri commands `start_google_oauth`, `get_google_auth_status`, `disconnect_google_auth`.
  - `src-tauri/src/managers/google_client.rs`: Maintain OAuth tokens, auto-refresh them, construct RFC 2822 emails for Gmail send API, and insert tasks into Google Tasks default list.
  - `src-tauri/src/lib.rs`: Hook up new Google-related Tauri commands and manage lifecycle/state.
- **React Frontend**:
  - `src/components/settings/meetings/MeetingsSettings.tsx`: Google Services connection section and "Send via Google" button for each meeting entry.
  - Recipient email address input dialog for choosing who receives the email.

## Milestones

| #   | Name                          | Scope                                                                                             | Dependencies | Status      |
| --- | ----------------------------- | ------------------------------------------------------------------------------------------------- | ------------ | ----------- |
| 1   | E2E Test Suite                | Build E2E testing infra and write playwright tests (Tiers 1-4)                                    | None         | DONE        |
| 2   | Backend Google Integration    | Implement local OAuth 2.0 flow, token storage, and reqwest clients/Tauri commands for Gmail/Tasks | M1           | PLANNED     |
| 3   | Frontend UI & Integration     | Connect OAuth status, recipient dialog, send follow-up commands, and feedback toast notifications | M2           | PLANNED     |
| 4   | E2E Integration Pass          | Pass all E2E test tiers and verify end-to-end integration                                         | M3           | PLANNED     |
| 5   | Adversarial Hardening         | Implement Tier 5 tests, identify/resolve gaps, final validation                                   | M4           | PLANNED     |

## Interface Contracts

### Tauri Commands

- `start_google_oauth() -> Result<String, String>`: Start local loopback TCP listener and open default browser to OAuth page. Return connection result/status.
- `get_google_auth_status() -> Result<bool, String>`: Check if refresh token exists in AppSettings.
- `disconnect_google_auth() -> Result<(), String>`: Clear token from AppSettings.
- `send_meeting_follow_up(recipients: Vec<String>, summary: String, action_items: Vec<String>) -> Result<(), String>`: Send email via Gmail and create tasks in Google Tasks.

## Code Layout

- `src-tauri/src/commands/google_auth.rs`: Tauri commands for OAuth
- `src-tauri/src/managers/google_client.rs`: Google API client and token manager
- `src/components/settings/meetings/`: Contains Google Services connection UI and meeting summary viewer integrations

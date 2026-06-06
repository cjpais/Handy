# MASR E2E Test Infrastructure

This document outlines the End-to-End (E2E) testing infrastructure for the MASR application, including the Tauri IPC mock layer, test design tiers, and instructions on running and writing tests.

---

## Architecture Overview

MASR is a Tauri-based application, running a Rust backend and a React/TypeScript frontend. In a standard E2E environment (like Playwright), Tauri's custom IPC backend (`window.__TAURI_INTERNALS__`) is not available because the application runs inside a regular web browser context rather than the Tauri WebView wrapper.

To run genuine E2E tests without running a full compiled Tauri binary, we inject a custom Tauri IPC mock layer before the page loads.

### Mock Strategy (`tests/helpers.ts`)

1. **Tauri IPC Injection**:
   Using Playwright's `page.addInitScript()`, we inject a mock `window.__TAURI_INTERNALS__` object that intercepts Tauri IPC command invokes (`invoke`).
2. **State Management**:
   We maintain a mock state (`MockState`) in the browser context that stores the state of Google Services connection (`googleConnected`), OAuth flow success/failure flags, follow-up metadata, and output language.
3. **Session Persistence**:
   To prevent state loss during page reloads (e.g., `page.reload()` in settings persistence tests), we serialize the mock state into the browser's `sessionStorage` and restore it during initialization.
4. **Mocked Tauri Commands**:
   - `get_app_settings` / `get_default_settings`: Returns app settings reflecting the mocked state.
   - `change_output_language_setting`: Updates the language in the state.
   - `get_google_auth_status`: Returns authentication status.
   - `start_google_oauth`: Updates connection status on success or returns OAuth failure.
   - `disconnect_google_auth`: Resets Google Services connection.
   - `send_meeting_follow_up`: Captures parameters (recipients, summary, action items) into a global mock state so tests can assert correct payloads.
   - `has_any_models_available`, `get_available_models`, `get_current_model`, `get_model_info`: Mocks the local model store configuration.
   - `get_available_microphones` / `get_available_output_devices`: Returns mock CPAL audio devices.
   - `get_windows_microphone_permission_status`: Returns granted/allowed permission status.
   - Tauri system plugins like `plugin:event|listen`, `plugin:event|unlisten`, `initialize_enigo`, `initialize_shortcuts`, `show_main_window_command`, and `__TAURI_OS_PLUGIN_INTERNALS__` are also stubbed to avoid runtime errors.

---

## Testing Tiers

Our E2E suite organizes Google Services integration tests into four robust testing tiers:

### Tier 1: Basic Integration & Operations

Verifies the happy path of Google Services integration:

- **Connect Google Services**: Triggers OAuth and validates the UI transitions to "Connected".
- **Send Follow-Up**: Opens the email form, fills recipients, clicks send, and verifies the backend command is invoked with correct summary and action items.
- **Disconnect Google Services**: Resets the state and asserts the UI returns to the disconnected prompt.

### Tier 2: Error Boundaries & Input Validation

Verifies robust handling of negative scenarios:

- **Input Validation**: Asserts empty input or malformed email patterns block submission and display appropriate error labels.
- **OAuth Failure**: Simulates external OAuth workflow failures and confirms the application remains disconnected.
- **API Failure**: Simulates downstream Google API errors during follow-up transmission, verifying the modal dialog remains open with the state preserved and error messages are handled gracefully.

### Tier 3: UI State & Post-processing Sync

Verifies dynamic state transitions:

- **Button Visibility**: Asserts the "Send via Google" button is dynamically hidden/displayed in real time depending on Google Services connection status.
- **Post-processing Sync**: Confirms that follow-ups sent match the exact post-processed meeting summary and action items of the active history item.

### Tier 4: Workload & Loading States

Verifies scalability and real-world execution:

- **Delimiter Parsing**: Validates the recipient email field parses multiple space- or comma-separated addresses correctly.
- **Workload Handling**: Verifies correct handling of multi-recipient payloads and mock loading states.

---

## Instructions to Run Tests

### Prerequisites

- Node.js / Bun installed
- Project dependencies installed via `bun install`

### Running the Test Suite

Playwright is configured to automatically launch the Vite development server on `http://localhost:1420` before starting the tests.

1. **Run all E2E tests**:

   ```bash
   bun run test:playwright
   ```

2. **Run E2E tests in UI mode** (provides a visual interface to inspect DOM states, console logs, and step-by-step trace):

   ```bash
   bun run test:playwright:ui
   ```

3. **Format code styles** (ensure code conforms to Prettier rules before contributing):
   ```bash
   bun run format
   ```

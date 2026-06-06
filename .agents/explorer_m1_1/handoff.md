# Handoff Report - explorer_m1_1

## 1. Observation

- **E2E Testing Infrastructure**:
  - `playwright.config.ts` is configured to run tests against the Vite dev server at `http://localhost:1420` (line 51: `baseURL: "http://localhost:1420"`) using a headless Chromium browser project (line 56).
  - Existing Playwright test files reside under the `tests/` directory: `tests/app.spec.ts` (basic server and page checks) and `tests/output_language.spec.ts` (interacts with general settings output language options).
- **Verbatim Playwright Test Failure**:
  - Running `bun run test:playwright` outputs a failure in `output_language.spec.ts`:

    ```
    1) [chromium] › tests\output_language.spec.ts:4:3 › Output Language Settings › allows selecting different output languages

      Error: expect(locator).toBeVisible() failed

      Locator: locator('text=Output Language')
      Expected: visible
      Timeout: 5000ms
      Error: element(s) not found
    ```

- **Zustand Store and Onboarding Flow**:
  - In `src/stores/settingsStore.ts`, the Zustand store initializes the `settings` state to `null` (line 48). If the Tauri backend is not running, the `commands.getAppSettings()` invocation throws an error, leaving `settings` as `null` (lines 405-408).
  - In `src/App.tsx`, `checkOnboardingStatus()` calls `commands.hasAnyModelsAvailable()` (line 155). If this throws an error (which it does due to the missing backend), the application catches the error and sets `onboardingStep` to `"accessibility"` (lines 167-170).
  - When `onboardingStep !== "done"`, `App.tsx` renders ONLY the onboarding screens (lines 182-193), blocking the settings UI from rendering.
- **Tauri Mocking Contract**:
  - In `node_modules/@tauri-apps/api/mocks.js`, the mocking system injects mock handlers into `window.__TAURI_INTERNALS__.invoke` (line 160) and provides event callback registrations through `transformCallback` and `runCallback` (lines 161-163).
- **Google Services Requirements**:
  - `PROJECT.md` outlines the following interface contracts for new Google-related Tauri commands:
    - `start_google_oauth() -> Result<String, String>`
    - `get_google_auth_status() -> Result<bool, String>`
    - `disconnect_google_auth() -> Result<(), String>`
    - `send_meeting_follow_up(recipients: Vec<String>, summary: String, action_items: Vec<String>) -> Result<(), String>`

## 2. Logic Chain

- **Premise 1**: Playwright E2E tests run against the browser-based Vite dev server, which is detached from the Rust backend.
- **Premise 2**: Without the backend, any Tauri command invocation throws an error, prompting the frontend settings store to keep its state `null` and forcing the app to display onboarding pages instead of the settings panel.
- **Premise 3**: This causes E2E tests looking for settings elements to fail because the settings screen is blocked by onboarding.
- **Premise 4**: By utilizing Playwright's `page.addInitScript(...)`, we can inject a mock Tauri command handler (`window.__TAURI_INTERNALS__.invoke`) and event registration stubs prior to page load.
- **Premise 5**: Stabbing these commands (simulating that models are present and onboarding is complete) allows the app to bypass onboarding and display settings pages.
- **Premise 6**: Mocking the new Google services commands will enable Playwright to simulate OAuth connect/disconnect flows and confirm the validation/sending of meeting summaries.

## 3. Caveats

- **Opaque-Box Limitations**: The frontend mocks simulate the Tauri IPC commands at the Javascript layer. They do not run the actual Rust backend code. Real-world end-to-end verification must occur in a packaged Tauri environment (Milestone 5).
- **Vite Dev Server Dependencies**: The E2E tests rely on Vite compiling and serving the frontend. Slow compilation or local environment latency may require adjusting Playwright's action or expect timeouts.

## 4. Conclusion

We propose the following plan for the E2E Test Suite implementation:

### 1. Mocking Tauri Commands

Inject the mock IPC handlers during test setup using `page.addInitScript`:

```typescript
test.beforeEach(async ({ page }) => {
  await page.addInitScript(() => {
    window.__TAURI_INTERNALS__ = window.__TAURI_INTERNALS__ || {};

    // Internal state simulation
    const state = {
      google_refresh_token: null as string | null,
    };

    // Global listener mapping to trigger events
    window.__TAURI_EVENT_LISTENERS__ =
      window.__TAURI_EVENT_LISTENERS__ || new Map();

    window.__TAURI_INTERNALS__.transformCallback = (cb, once) => {
      const id = Math.floor(Math.random() * 1000000);
      window.__TAURI_CALLBACKS__ = window.__TAURI_CALLBACKS__ || new Map();
      window.__TAURI_CALLBACKS__.set(id, (payload) => {
        if (once) window.__TAURI_CALLBACKS__.delete(id);
        cb(payload);
      });
      return id;
    };

    window.__TAURI_INTERNALS__.invoke = async (cmd, args) => {
      switch (cmd) {
        case "get_app_settings":
          return {
            status: "ok",
            data: {
              output_language: "malayalam",
              bindings: {},
              push_to_talk: false,
              audio_feedback: false,
              google_refresh_token: state.google_refresh_token,
            },
          };
        case "has_any_models_available":
          return { status: "ok", data: true };
        case "plugin:event|listen":
          const { event, handler } = args;
          if (!window.__TAURI_EVENT_LISTENERS__.has(event)) {
            window.__TAURI_EVENT_LISTENERS__.set(event, []);
          }
          window.__TAURI_EVENT_LISTENERS__.get(event).push(handler);
          return handler;
        // Mock Google-related commands
        case "get_google_auth_status":
          return state.google_refresh_token !== null;
        case "start_google_oauth":
          state.google_refresh_token = "mock-refresh-token";
          return "success";
        case "disconnect_google_auth":
          state.google_refresh_token = null;
          return;
        case "send_meeting_follow_up":
          // Log or register execution for test assertions
          window.__LAST_FOLLOW_UP__ = args;
          return;
        default:
          return { status: "ok", data: null };
      }
    };
  });
});
```

### 2. E2E Test Suite Design (`tests/google_integration.spec.ts`)

- **Tier 1 (Feature Coverage)**:
  - Connect Google Services: Simulates clicking "Connect" -> verifies `start_google_oauth` is invoked -> checks UI transitions to "Connected".
  - Disconnect Google Services: Simulates clicking "Disconnect" -> verifies `disconnect_google_auth` is invoked -> checks UI transitions to "Disconnected".
  - Send Follow-Up: Connects account -> triggers custom event -> fills recipient email dialog -> clicks "Send via Google" -> asserts `send_meeting_follow_up` was called with correct arguments.
- **Tier 2 (Boundary & Corner Cases)**:
  - Empty Recipient Input: Attempting to confirm follow-up dialog with no recipients displays error and blocks command.
  - Invalid Email Validation: Typing `invalid-email` displays inline/toast validation.
  - OAuth Failure: Mocks `start_google_oauth` returning error -> asserts UI displays failure toast and remains disconnected.
  - Send Follow-Up Failure: Mocks `send_meeting_follow_up` returning error -> asserts UI displays error toast and leaves dialog open.
- **Tier 3 (Cross-Feature Interactions)**:
  - Disconnected State Button Visibility: Verify "Send via Google" button is either hidden or prompts login when disconnected.
  - Post-Processing Integration: Verify follow-up details (summary & action items) match the current meeting's post-processing settings/output.
- **Tier 4 (Real-World Workloads)**:
  - Large Meeting Transcript Follow-Up: Simulates multiple paragraphs of summary, a long list of action items, and multiple recipient emails -> clicks Send -> checks loading/spinner state -> asserts correct call parameters.

### 3. Suggested `TEST_INFRA.md` Structure

- **Overview**: Dual architecture of Tauri testing (Rust backend + browser E2E).
- **Playwright E2E Setup**: Configuration details, command run guides (`bun run test:playwright`), and local setup checklist.
- **Tauri Mocking Framework**: Detailed guide on how to use the browser-level mock layer (`page.addInitScript`), and how to add new mock command mappings.
- **Backend Rust Mocks**: Overview of backend tests (`cargo test`) and the CI file-swapping strategy for `TranscriptionManager`.
- **Test Tiers Reference**: Explanation of Tiers 1-5, mapping requirements to each tier.

## 5. Verification Method

- Run Playwright E2E tests:
  ```bash
  bun run test:playwright
  ```
- Verify `tests/output_language.spec.ts` failures are resolved when implementing the app settings stubs, and that the new `tests/google_integration.spec.ts` executes successfully.

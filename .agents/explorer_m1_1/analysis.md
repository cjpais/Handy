# Analysis - Google Integration E2E Test Suite Design

## 1. Observations

- **E2E Testing Infrastructure Configuration**:
  - The project has a Playwright configuration file at `playwright.config.ts`. It runs tests against the Vite dev server on `http://localhost:1420` (line 51: `baseURL: "http://localhost:1420"`). It targets Chromium as the testing browser (line 56).
  - Existing E2E tests are located in the `tests/` directory:
    - `tests/app.spec.ts`: Contains basic connectivity tests checking if the dev server responds and if the root HTML element `#root` is present.
    - `tests/output_language.spec.ts`: Tests output language selection by clicking language selector buttons and asserting style changes (e.g. `bg-logo-primary` class presence).
- **Verbatim Playwright Test Failure**:
  - Running `bun run test:playwright` outputs the following error for `output_language.spec.ts`:

    ```
    1) [chromium] › tests\output_language.spec.ts:4:3 › Output Language Settings › allows selecting different output languages

      Error: expect(locator).toBeVisible() failed

      Locator: locator('text=Output Language')
      Expected: visible
      Timeout: 5000ms
      Error: element(s) not found

      Call log:
        - Expect "toBeVisible" with timeout 5000ms
        - waiting for locator('text=Output Language')

         8 |     // Verify Output Language setting group exists by checking for its title
         9 |     const titleLocator = page.locator("text=Output Language");
      > 10 |     await expect(titleLocator).toBeVisible();
           |                                ^
        11 |
        12 |     // Verify option buttons are visible
        13 |     const malayalamBtn = page.locator("button:has-text('Malayalam')");
          at D:\Downloads\Projects\MASR\tests\output_language.spec.ts:10:32
    ```

- **Frontend State Management & Onboarding Flow**:
  - The frontend settings state is managed in `src/stores/settingsStore.ts` using Zustand. The initial state of `settings` is `null` (line 48).
  - On mount, `settingsStore.ts` calls `initialize()` (line 457) which executes `refreshSettings()` (line 387). `refreshSettings` makes a Tauri IPC call `commands.getAppSettings()` (line 389).
  - If the Tauri backend is not running, the IPC call throws an error. The store catches this error, prints `"Failed to load settings"`, and leaves `settings` as `null` (lines 405-408).
  - In `src/App.tsx`, `checkOnboardingStatus()` calls `commands.hasAnyModelsAvailable()` (line 155). Since there is no running Tauri backend and no mocked IPC, this call throws an error. The error is caught, and `onboardingStep` is set to `"accessibility"` (lines 167-170).
  - When `onboardingStep` is not `"done"`, `App.tsx` renders ONLY the onboarding screens (lines 182-193). The main interface (including Settings pages and the Output Language Selector) is never mounted or rendered, causing the settings elements to be missing and the test to fail.
- **Google Integration Architecture**:
  - The integration plan (`PROJECT.md`) defines four new Tauri commands for Google services:
    - `start_google_oauth() -> Result<String, String>`
    - `get_google_auth_status() -> Result<bool, String>`
    - `disconnect_google_auth() -> Result<(), String>`
    - `send_meeting_follow_up(recipients: Vec<String>, summary: String, action_items: Vec<String>) -> Result<(), String>`

## 2. Logic Chain

- **Premise 1**: Playwright E2E tests run against a standard Chromium browser pointing to the Vite dev server, which runs independently of the Rust Tauri application.
- **Premise 2**: Since the Rust Tauri application is not running during E2E tests, any Tauri IPC commands (imported from `@tauri-apps/api/core` via `@/bindings`) will throw uncaught errors because `window.__TAURI_INTERNALS__` is undefined.
- **Premise 3**: Uncaught IPC errors during frontend initialization cause the settings store to remain `null` and force the application into the accessibility onboarding flow, hiding the settings panel. This is confirmed by the verbatim test failure where `locator('text=Output Language')` cannot be found.
- **Premise 4**: Consequently, tests checking for settings elements (like the output language selectors) fail because the elements are not present on the onboarding screen.
- **Premise 5**: To allow Playwright tests to navigate the main settings UI and test settings updates, we must stub Tauri commands.
- **Premise 6**: Tauri's frontend library delegates `invoke` calls to `window.__TAURI_INTERNALS__.invoke`. Therefore, we can inject a mock IPC handler by setting `window.__TAURI_INTERNALS__.invoke` using Playwright's `page.addInitScript(...)` before navigating to the page.
- **Premise 7**: We can extend this mock handler to simulate Google OAuth states, disconnect flows, and follow-up actions to test Tiers 1-4 of the Google Integration E2E suite.

## 3. Caveats

- **CI-only backend mocks**: The Rust backend uses a file-swap mock strategy for `TranscriptionManager` (using `transcription_mock.rs` in CI) to skip CUDA/Vulkan dependencies. This is distinct from the frontend E2E mock strategy, which operates entirely at the browser/JS level.
- **Token expiration mocking**: Simulating token expiration in E2E tests requires either changing the mocked Tauri response dynamically mid-test or simulating a backend event. Our proposed design supports both by utilizing helper injection on the `window` object.

## 4. Conclusion

- We must implement a robust frontend mocking strategy using Playwright's `page.addInitScript(...)` to intercept Tauri IPC calls in E2E tests.
- This mocking layer should cover:
  1. App settings loading and saving.
  2. Onboarding status check commands (simulating that models exist and onboarding is complete).
  3. Platform permissions/microphones enumeration.
  4. Google OAuth and follow-up commands.
- This design will enable the successful execution of the Tiers 1-4 E2E test cases in `tests/google_integration.spec.ts`.

## 5. Verification Method

- The E2E tests can be run locally using the project test command:
  ```bash
  bun run test:playwright
  ```
- To verify the mock implementation, check that `tests/output_language.spec.ts` and the new `tests/google_integration.spec.ts` pass when executed in headless Chromium.

# Scope: E2E Test Suite (Milestone 1)

## Architecture

- The test suite must be opaque-box, requirement-driven, and run via Playwright.
- Test files should be placed under `tests/` directory (e.g., `tests/google_integration.spec.ts`).
- We need to configure test runners and mock Google API endpoints if necessary, or design the tests to interact with mock backend state.
- Create `TEST_INFRA.md` at project root.

## Milestones

| #   | Name                                | Scope                                                                      | Dependencies | Status  |
| --- | ----------------------------------- | -------------------------------------------------------------------------- | ------------ | ------- |
| 1.1 | Test Infra Setup                    | Design and set up Playwright test files and infrastructure                 | None         | DONE    |
| 1.2 | Feature Coverage (Tier 1)           | Write tests for main features: OAuth connect/disconnect and send follow-up | 1.1          | DONE    |
| 1.3 | Boundary & Corner Cases (Tier 2)    | Write tests for empty inputs, invalid emails, expired tokens               | 1.2          | DONE    |
| 1.4 | Cross-Feature Interactions (Tier 3) | Write tests combining OAuth states and post-processing                     | 1.3          | DONE    |
| 1.5 | Real-World Workloads (Tier 4)       | Write tests for end-to-end flow with large meeting transcript              | 1.4          | DONE    |

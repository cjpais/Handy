## 2026-06-05T12:55:17Z

Please implement the API Key testing backend command and frontend UI button in the MASR codebase.

Your working directory is d:/Downloads/Projects/MASR/.agents/worker_api_key.
You must update your heartbeat in d:/Downloads/Projects/MASR/.agents/worker_api_key/progress.md after each step.

### Detailed Requirements:

1. **Backend Command Update**:
   - Locate `test_post_process_api_key` in `src-tauri/src/commands/mod.rs`.
   - Update its signature to accept only `provider_id: String` (plus `AppHandle` or `app: AppHandle`) and return `Result<String, String>`.
   - Inside the command, load the API key from settings using `settings.post_process_api_keys.get(&provider_id)`.
   - Run the validation request via `crate::llm_client::fetch_models(provider, api_key).await`.
   - Return `Ok(String)` containing a descriptive success message (e.g., indicating the number of models retrieved) or `Err(String)` containing the validation error.
   - For `apple_intelligence`, immediately return `Ok(String)` stating it is configured locally and requires no API key.

2. **React API Key Field Update**:
   - Locate `src/components/settings/PostProcessingSettingsApi/ApiKeyField.tsx`.
   - Add an optional `onChange?: (value: string) => void` property to `ApiKeyFieldProps`.
   - In the input element's `onChange`, call both the local state setter and the new `onChange` prop.

3. **React Post-Processing Settings Integration**:
   - Locate `src/components/settings/post-processing/PostProcessingSettings.tsx`.
   - In `PostProcessingSettingsApiComponent`, use a local state `localApiKey` initialized with `state.apiKey` and kept in sync via `useEffect` when `state.apiKey` changes.
   - Pass `onChange={setLocalApiKey}` to `<ApiKeyField>`.
   - Enable the "Test" button if `localApiKey` is not empty.
   - Inside `handleTestApiKey`:
     - Import `updatePostProcessApiKey` from `useSettings()`.
     - If `localApiKey !== state.apiKey`, execute and `await updatePostProcessApiKey(state.selectedProviderId, localApiKey)`.
     - Then invoke `await commands.testPostProcessApiKey(state.selectedProviderId)`.
     - Display the success message returned in the `Ok` variant (i.e. `result.data`) or show the error from the `Err` variant (i.e. `result.error`).

4. **Verify Build & Generate TypeScript Bindings**:
   - Compile the Rust backend in `src-tauri/`. Note that the project is configured to build target to `C:\t` via `src-tauri/.cargo/config.toml`.
   - Run the compiled debug binary briefly (e.g., execute `C:\t\debug\handy-app.exe` or `C:\t\debug\handy.exe` for a second and then terminate/kill it) to trigger the `specta_builder` export code in `lib.rs` and update `src/bindings.ts`.
   - Run `bun run lint` and `bun run build` to ensure the frontend compiles without errors.

MANDATORY INTEGRITY WARNING:
DO NOT CHEAT. All implementations must be genuine. DO NOT hardcode test results, create dummy/facade implementations, or circumvent the intended task. A Forensic Auditor will independently verify your work. Integrity violations WILL be detected and your work WILL be rejected.

When completed, write a handoff report at `d:\Downloads\Projects\MASR\.agents\worker_api_key\handoff.md` and send a message back.

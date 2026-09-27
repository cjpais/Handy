# `src/` — React/TypeScript frontend

The frontend for both Tauri windows: the main settings window and the recording
overlay. Built with Vite 6, React 18, TypeScript 5.6 (strict), Tailwind CSS 4 and
Zustand. All UI strings go through i18next.

For how the frontend fits into the whole app, see the "Frontend map" section of
[`codebase-v1.md`](../context/references/codebase-v1.md).

---

## Two entry points, two React roots

`tauri.conf.json` declares `"windows": []`. **Both windows are created at runtime
in Rust**, and each loads a separate HTML entry:

| Entry | HTML | Path alias | Window |
| --- | --- | --- | --- |
| `src/main.tsx` | `index.html` | `@/` → `./src/` | `main` (settings) |
| `src/overlay/main.tsx` | `src/overlay/index.html` | same | `recording_overlay` |

`vite.config.ts` declares both as Rollup inputs. The consequence: **two
independent module graphs and duplicated boot work**. Anything imported by both
entries is loaded twice, so keep shared boot code small.

`main.tsx` order is deliberate — `installCompatShims()` (polyfills `Object.hasOwn`
for react-markdown), set `data-platform` on `<html>` for platform-scoped CSS,
apply the cached theme *before* render to avoid a flash of the wrong palette,
import i18n, initialise the model store, then render inside `<StrictMode>`.

---

## Layout

| Path | Purpose |
| --- | --- |
| `App.tsx` | Onboarding gate + section switch. **No router.** |
| `main.tsx`, `overlay/main.tsx` | The two React roots |
| `bindings.ts` | **Generated** by tauri-specta — never edit by hand |
| `components/` | UI, grouped by area (see below) |
| `stores/` | Zustand stores: `settingsStore`, `modelStore` |
| `hooks/` | `useSettings` (facade over the store), `useOsType` |
| `i18n/` | i18next setup, language metadata, 27 locale JSON files |
| `lib/` | `constants/`, `types/`, `utils/` — framework-free helpers |
| `overlay/` | Overlay window component, entry and its own CSS |
| `styles/theme.css` | Palette source of truth (light/dark + `data-theme` overrides) |
| `content/release-notes/` | Markdown release notes, globbed at build time |
| `utils/dateFormat.ts` | Date formatting (distinct from `lib/utils/format.ts`) |

### `components/`

- `settings/` — the bulk of the UI. Group components are `general/`, `models/`,
  `history/`, `post-processing/`, `advanced/`, `about/`, `debug/`, each assembling
  small leaf settings (e.g. `AudioFeedback.tsx` is 33 lines).
- `ui/` — shared primitives. `index.ts` exports only nine (Dropdown, Dialog,
  Slider, ToggleSwitch, SettingContainer, SettingsGroup, TextDisplay, Textarea,
  Tooltip); `Button`, `Input`, `Badge`, `Alert`, `ResetButton`, `PathDisplay` and
  `AudioPlayer` are imported by direct path.
- `onboarding/`, `update-checker/`, `whats-new/`, `shared/`, `footer/`, `icons/`.
- `model-selector/` — **legacy parallel model UI** mounted only from
  `footer/Footer.tsx`. It duplicates `settings/models/`; two model UIs exist and
  must be kept consistent.

---

## Conventions

**Section registration.** `components/Sidebar.tsx` owns `SECTIONS_CONFIG`, the
single registry describing every settings section: `{ labelKey, icon, component,
enabled(settings) }`. `SidebarSection` is derived from its keys. To add a section,
add one entry — nothing else.

**The leaf-setting pattern** is control → `SettingContainer` (title, description,
`descriptionMode: "inline" | "tooltip"`, `grouped`, `disabled`) → optionally
`ToggleSwitch`, which shows a spinner while updating.

**Adding a new setting** requires six coordinated edits. Missing step 4 is the
most common mistake:

1. Add the field and default in `src-tauri/src/settings.rs`.
2. Add a `change_*_setting` command and register it in the `collect_commands![...]`
   list in `src-tauri/src/lib.rs`.
3. Run `bun run tauri dev` to regenerate `src/bindings.ts`. Bindings are exported
   **only in debug builds**, so a release build will not regenerate them.
4. Add an entry to `settingUpdaters` in `stores/settingsStore.ts`.
   **Without it, `updateSetting` only logs "No handler for setting" and the UI
   silently diverges from Rust.** (`bindings` and `selected_model` are
   intentionally excluded — they have dedicated commands.)
5. Create `components/settings/X.tsx` following `AudioFeedback.tsx` and export it
   from `components/settings/index.ts`.
6. Add keys to `i18n/locales/en/translation.json`, then run
   `bun run check:translations`.

**IPC.** Call `commands.someCommand(...)` from `@/bindings`. The return shape is
`{ status: "ok", data } | { status: "error", error }`, but genuine JS exceptions
are **re-thrown**, so callers must handle both rejection and `status: "error"`.
Only three events are typed (`historyUpdatePayload`, `streamTextEvent`,
`streamPhaseEvent`); everything else is a raw `listen("...")` string, and payloads
mirrored by hand in `lib/types/events.ts` **will drift silently** if the Rust
structs change.

**i18n.** No hardcoded user-facing strings. ESLint's `i18next/no-literal-string`
runs with `markupOnly: true`, so **strings passed as JS props are not checked** —
do not treat a clean lint run as proof. Add keys to the `en` locale first.

**Styling.** Tailwind 4 with logical properties (`border-e`, `me-2`) so RTL works.
The palette lives in `styles/theme.css`. The `@/` alias is declared in **both**
`tsconfig.json` and `vite.config.ts` — keep them in sync.

**Tests.** `lib/utils/keyboard.test.ts` and friends are plain `bun` scripts using
`node:assert`, **not** vitest. There is no frontend unit-test runner wired up
beyond `test:keyboard`.

---

## Traps

- **Adding a setting without a `settingUpdaters` entry** silently desynchronises
  the UI from Rust. See step 4 above.
- `stores/settingsStore.ts` sets `isLoading: false` even when loading fails, so a
  failed load renders the app with `settings === null` and components fall back to
  `?? false` defaults that may not match Rust.
- `subscribeWithSelector` is enabled on both stores but **no `subscribe()` call
  exists anywhere** — currently vestigial.
- `settings/debug/DebugPaths.tsx` is **dead code** (never imported, absent from
  `debug/index.ts`). `vite-env.d.ts` is likewise unreferenced.
- Two unrelated language tables must not be merged: `i18n/languages.ts` (UI
  locales) and `lib/constants/languages.ts` (model capability intents).
- Two format utilities: `utils/dateFormat.ts` and `lib/utils/format.ts`.
- `App.tsx` mixes `./...` and `@/...` import styles; both work.
- The overlay's `--ov-*` CSS variables in `overlay/RecordingOverlay.css` must stay
  in sync with the window sizing in `src-tauri/src/overlay.rs`. The CSS imports
  `theme.css` by **relative** path because CSS cannot use the `@/` alias.

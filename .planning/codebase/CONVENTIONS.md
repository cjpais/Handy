# Coding Conventions

**Analysis Date:** 2026-03-28

## Naming Patterns

**Files:**
- React components: PascalCase `.tsx` — `AudioFeedback.tsx`, `ToggleSwitch.tsx`
- Hooks: camelCase prefixed with `use` — `useSettings.ts`, `useModels.ts`
- Stores: camelCase suffixed with `Store` — `settingsStore.ts`
- Utilities/lib: camelCase — `rtl.ts`, `utils.ts`
- Rust modules: snake_case — `audio.rs`, `transcription.rs`, `signal_handle.rs`

**Functions/Variables:**
- TypeScript: camelCase for functions and variables — `updateSetting`, `refreshAudioDevices`, `audioFeedbackEnabled`
- React hooks: `use` prefix — `useSettings`, `useModels`
- Rust: snake_case throughout — `get_app_settings`, `send_transcription_input`

**Types/Interfaces:**
- TypeScript: PascalCase — `UseSettingsReturn`, `AudioFeedbackProps`, `ValidationResult`
- Interface props suffix: `Props` — `AudioFeedbackProps`
- Rust structs/enums: PascalCase — `LogLevel`, `ShortcutBinding`, `AppSettings`

**Constants:**
- TypeScript: SCREAMING_SNAKE_CASE — `SUPPORTED_LANGUAGES`, `REFERENCE_LANG`
- Rust: SCREAMING_SNAKE_CASE — `APPLE_INTELLIGENCE_PROVIDER_ID`, `APPLE_INTELLIGENCE_DEFAULT_MODEL_ID`

## Code Style

**Formatting:**
- Tool: Prettier (frontend) + `cargo fmt` (backend), run together via `bun run format`
- Config: `C:/Users/pc/dev/Handy/.prettierrc` — only `"endOfLine": "lf"` specified; all other settings are Prettier defaults
- Rust edition: 2021 (`C:/Users/pc/dev/Handy/src-tauri/rustfmt.toml`)
- Line endings: LF enforced

**Linting:**
- Tool: ESLint 9 with flat config (`C:/Users/pc/dev/Handy/eslint.config.js`)
- Parser: `@typescript-eslint/parser`
- Key rule: `i18next/no-literal-string` (error, `markupOnly: true`) — all JSX text content must use `t()` translations
- Ignored attributes: `className`, `style`, `type`, `id`, `name`, `key`, `data-*`, `aria-*`
- Rust: `cargo clippy` before committing

**TypeScript Strictness:**
- `"strict": true` in `tsconfig.json`
- `noFallthroughCasesInSwitch: true`
- `noUnusedLocals` and `noUnusedParameters` are disabled (not enforced)
- Avoid `any` types (project guideline)

## Import Organization

**Order (TypeScript):**
1. React and external packages — `import React from "react"`, `import { useTranslation } from "react-i18next"`
2. Internal path-alias imports — `import { commands } from "@/bindings"`
3. Relative imports — `import { useSettings } from "../../hooks/useSettings"`

**Path Aliases:**
- `@/` → `./src/` (e.g., `@/bindings`, `@/lib/utils/rtl`)
- `@/bindings` → `./src/bindings.ts` (auto-generated Tauri type bindings)

## Component Design

**Pattern:** Functional components with hooks only — no class components.

**Memoization:** `React.memo()` used on settings components to prevent unnecessary re-renders:
```tsx
export const AudioFeedback: React.FC<AudioFeedbackProps> = React.memo(
  ({ descriptionMode = "tooltip", grouped = false }) => { ... }
);
```

**Props interface:** Always define a named `Props` interface above the component:
```tsx
interface AudioFeedbackProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}
```

**Hook usage in components:**
```tsx
const { t } = useTranslation();
const { getSetting, updateSetting, isUpdating } = useSettings();
```

## Internationalization (i18n)

**Rule:** All user-facing text in JSX must use `t()` — enforced by ESLint (`i18next/no-literal-string`).

**Pattern:**
```tsx
const { t } = useTranslation();
// In JSX:
label={t("settings.sound.audioFeedback.label")}
description={t("settings.sound.audioFeedback.description")}
```

**Key structure:** Dot-notation namespaced keys — `settings.sound.audioFeedback.label`

**Adding new text:**
1. Add key to `src/i18n/locales/en/translation.json` (English is the reference/source locale)
2. Add the same key to all other locale files: `es`, `fr`, `vi` (and any others in `src/i18n/locales/`)
3. Run `bun run check:translations` to validate all locales are in sync

**Locale files:** `src/i18n/locales/{lang}/translation.json`

**Translation check script:** `scripts/check-translations.ts` — validates all locales have keys matching `en`

**Language detection:** System locale auto-detection via `@tauri-apps/plugin-os`, with fallback to `en`

## Error Handling

**TypeScript:**
- Tauri commands return `{ status: "ok", data: T } | { status: "error", error: E }` — always check `result.status`
- Use `try/catch` with `console.warn` for non-critical async operations (e.g., language sync failures)
- Prefer explicit error handling over silent failures

**Rust:**
- Use `Result<T, E>` returns — avoid `.unwrap()` in production code
- Log with `log::warn!` for recoverable errors, `log::debug!` for diagnostic info
- `#[derive(Serialize, Deserialize)]` with explicit `serde` attributes on all shared types

## State Management

**Pattern:** Zustand stores (`stores/settingsStore.ts`) consumed via custom hooks (`hooks/useSettings.ts`).

**Flow:** Component → custom hook → Zustand store → Tauri command → Rust state → `tauri-plugin-store` persistence

**Hook interface:** Hooks expose typed actions and state — never expose raw store methods directly to components.

## Styling

- Tailwind CSS utility classes — no CSS modules or styled-components
- Tailwind v4 (via `@tailwindcss/vite` plugin)
- Inline `className` strings

## Rust Conventions

- All types shared with the frontend derive `Serialize`, `Deserialize`, `Clone`, `Debug`, and `specta::Type`
- `#[serde(rename_all = "lowercase")]` or `"snake_case"` on enums/structs as appropriate
- `use log::{debug, warn}` for logging — never `println!` in production
- Doc comments (`///`) on public APIs
- Explicit `impl From<X> for Y` for type conversions

## Module/Export Pattern

- Named exports for components and utilities: `export const AudioFeedback`
- Default export only for singleton modules (e.g., `export default i18n`)
- No barrel `index.ts` re-exports detected — import directly from file paths

---

*Convention analysis: 2026-03-28*

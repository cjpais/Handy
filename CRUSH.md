# Development Commands

**Environment Setup:**

```bash
bun install                    # Install dependencies
mkdir -p src-tauri/resources/models
curl -o src-tauri/resources/models/silero_vad_v4.onnx https://blob.handy.computer/silero_vad_v4.onnx
```

**Development:**

```bash
bun run tauri dev              # Full app development
CMAKE_POLICY_VERSION_MINIMUM=3.5 bun run tauri dev  # macOS with cmake fix
bun run dev                     # Frontend only (Vite)
bun run build                   # Build frontend
bun run tauri build             # Production build
```

**Type Check & Build:**

```bash
bunx tsc --noEmit               # Type checking
bun run build                   # Build and validate
```

# Code Style Guidelines

**Rust (Backend):**

- Use `anyhow::Error` for error handling with descriptive messages
- Prefer `Arc<Mutex<T>>` for shared state in managers
- Log with appropriate levels: `debug!`, `info!`, `eprintln!` for errors
- Builder pattern for initialization chains
- Snake_case for functions and variables, PascalCase for types
- Separate logical sections with comment blocks: `/* ─────────── */`

**TypeScript/React (Frontend):**

- Functional components with TypeScript interfaces
- Zod schemas for type validation and inference
- `useCallback` hooks for stable function references
- Destructure props with defaults: `disabled = false`
- Prefer interface aliases over type aliases for objects
- React.FC for explicit component typing
- PascalCase for components, camelCase for variables/functions

**Imports:**

- Group imports: external libs, internal modules, relative imports
- Use type imports for TypeScript: `import type { Settings }`
- Named imports preferred over default exports

**Error Handling:**

- Frontend: Try/catch with user feedback, rollback optimistic updates
- Backend: `?` operator with anyhow context messages
- Log errors appropriately for debugging level

**Component Patterns:**

- Container component pattern for layout
- Composition over inheritance
- Prop drilling minimized with context where appropriate

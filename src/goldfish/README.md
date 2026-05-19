# Goldfish frontend modules

Goldfish-only React / TypeScript code lives here. Upstream Handy frontend code stays in its existing locations (`src/components/`, `src/stores/`, `src/hooks/`, etc.).

Composition pattern (route registry vs. `<GoldfishMount/>` slot) is deferred until the first Goldfish UI lands — see [`docs/fork-strategy.md`](../../docs/fork-strategy.md#5-frontend-composition-pattern).

See [`docs/scaffold.md`](../../docs/scaffold.md) for the broader scaffold rationale.

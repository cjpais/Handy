# Goldfish documentation

Personal documentation for the Goldfish project — a new application built on the technical foundation of [Handy](https://github.com/cjpais/Handy).

This folder is for **your** tracking: architecture notes, fork strategy, and decisions as the project evolves. It is separate from upstream Handy docs (`README.md`, `AGENTS.md`, `BUILD.md` at the repo root).

## Contents

| Document | Purpose |
|----------|---------|
| [codebase-overview.md](./codebase-overview.md) | What is in the repo today: stack, layout, runtime flow |
| [fork-strategy.md](./fork-strategy.md) | How to treat Handy as an engine, extend Goldfish, stay synced with upstream |
| [decisions.md](./decisions.md) | Log of product and technical decisions (update as you go) |

## Repo context

- **Upstream:** https://github.com/cjpais/Handy
- **This fork:** https://github.com/felixbaileymurray/goldfish
- **Local clone:** `Documents/goldfish` (folder name; product name is Goldfish)
- **Handy version at initial analysis:** 0.8.3

## Keeping docs current

When you make a significant choice (bundle ID, branch model, first Goldfish feature, upstream merge policy), add an entry to [decisions.md](./decisions.md) with date and rationale.

When upstream merges change architecture materially, skim [codebase-overview.md](./codebase-overview.md) and update the “last reviewed” note at the top.

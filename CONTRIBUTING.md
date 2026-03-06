# Contributing to the trading monorepo

Keep changes aligned with the repo ownership model.

## Start here

- Read [docs/repo/index.md](docs/repo/index.md) before making structural changes.
- Use [docs/repo/structure.md](docs/repo/structure.md) to decide where new code belongs.
- Use [docs/repo/workflows.md](docs/repo/workflows.md) when a change spans engine, system, app, ops, tooling, or research boundaries.
- Use [docs/repo/standards.md](docs/repo/standards.md) to avoid adding new structural debt.

## Contribution expectations

1. Keep changes small and ownership-consistent.
2. Put reusable runtime work in `engine/`.
3. Put strategy/runtime system work in `systems/`.
4. Put operator UI work in `apps/`.
5. Put deployment assets and runbooks in `ops/`.
6. Put developer, CI, and release automation in `tooling/`.
7. Put experiments and notebooks in `research/`.
8. Update the relevant docs whenever you move ownership boundaries, change contracts, or introduce compatibility shims.

## Structural migration rule

If a legacy path still exists for compatibility, do not add new implementation there. Update the canonical location and adjust the compatibility bridge only when needed.

## Review guidance

- Prefer single-source contracts over duplication.
- Prefer compatibility shims over flag-day renames.
- Prefer documented ownership boundaries over convenience imports.

# Repo structure

This repository is organized by ownership boundary, not by language alone.

## Top-level domains

- `engine/`: reusable runtime, Rust crates, Python bindings, schemas, and engine tests.
- `systems/`: deployable systems built on top of the engine.
- `apps/`: operator-facing applications and dashboards.
- `ops/`: deployment assets, runtime scripts, environment templates, and runbooks.
- `tooling/`: developer automation, CI entrypoints, and release scripts.
- `research/`: notebooks, experiments, examples, and exploratory work.
- `docs/repo/`: repo-wide governance for structure and workflow.

## Current canonical ownership

- `engine/` owns the `nautilus_trader` runtime identity.
- `systems/flux/` owns Flux strategy/runtime code and Flux-specific documentation.
- `apps/fluxboard/` and `apps/pulse-ui/` own the operator UI surfaces.
- `ops/` owns deployment wiring such as systemd units, env files, and operational shell scripts.
- `tooling/` owns developer, CI, and release automation.
- `research/` owns non-production examples and exploratory artifacts.

## Namespace split

- Engine code uses `nautilus_trader.*`.
- Flux code uses `flux.*`.
- `nautilus_trader.flux.*` exists only as a compatibility bridge during migration.

## Placement rules

Use these rules before adding a file or directory:

- Put reusable market/data/execution/core runtime code in `engine/`.
- Put strategy families, runtime orchestration, params, publishers, and system APIs in `systems/<system>/`.
- Put React/Vite/frontend clients in `apps/<app>/`.
- Put deploy/run/ops artifacts in `ops/`.
- Put developer, CI, and release automation in `tooling/`.
- Put notebooks, prototypes, benchmarks, and ad hoc analysis in `research/`.
- Do not create new top-level domains without updating `docs/repo/`.

## Recommended next cleanup steps

The current migration preserves compatibility paths. The next structure improvements should be:

1. Retire the legacy `scripts/` compatibility layer after internal callers move to canonical `tooling/` and `ops/` paths.
2. Introduce a `contracts/` domain if API payloads, socket schemas, or shared config schemas become reused across `systems/` and `apps/`.
3. Move additional app-specific docs beside the owning app once remaining links are updated.
4. Retire compatibility import paths after consumers stop relying on legacy module names.

## Anti-patterns

Avoid these placements:

- strategy-specific code under `engine/nautilus_trader`
- production imports from `research/`
- deployment scripts inside `apps/`
- UI-specific code inside `systems/`
- release or CI automation inside `ops/`
- copying the same contract definition into both app and system trees

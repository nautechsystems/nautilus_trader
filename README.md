# Trading Monorepo

This repository is the working monorepo for the trading stack.

At the product and deploy layer, the system is referred to as `flux`. At the engine layer, the runtime namespace remains `nautilus_trader`.

Use this naming split consistently:

- `flux`: product, repo, deploy lanes, Pulse groups, and operator-facing surfaces
- `flux.*`: Flux system/runtime implementation under `systems/flux/`
- `nautilus_trader.*`: engine/runtime imports and core platform identity

## Layout

- `engine/`: reusable runtime, Rust crates, Python bindings, schema, and engine tests.
- `systems/`: deployable trading systems built on the engine.
- `apps/`: operator-facing applications such as Fluxboard and Pulse UI.
- `ops/`: deployment assets, environment templates, operational scripts, and runbooks.
- `tooling/`: developer, CI, and release automation.
- `research/`: examples, experiments, notebooks, and ad hoc analysis artifacts.

## Runtime namespaces

- Use `nautilus_trader.*` for engine imports.
- Use `flux.*` for Flux strategy/runtime imports.
- `nautilus_trader.flux.*` remains available as a compatibility shim during migration.

## Canonical paths

- Use `systems/flux/flux/` for Flux runtime code and `systems/flux/docs/` for durable Flux docs.
- Use `apps/fluxboard/docs/` for Fluxboard-owned docs and runbooks. `apps/fluxboard` currently resolves to the existing top-level `fluxboard/` tree.
- Use `ops/scripts/` for operational scripts such as TokenMM risk audits and deploy helpers.
- Use `tooling/dev/`, `tooling/ci/`, and `tooling/release/` for version, CI, and release helpers.

## Migration note

Legacy root paths remain available while the repo transitions to the new structure. New work should update the canonical `systems/`, `apps/`, `ops/`, and `tooling/` locations first, then adjust compatibility shims only when needed.

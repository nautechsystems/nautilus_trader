# Trading Monorepo

This repository is the working monorepo for the trading stack.

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

- Use `systems/flux/docs/` for Flux durable docs.
- Use `apps/fluxboard/docs/` for Fluxboard-owned docs and runbooks.
- Use `ops/scripts/deploy/` for deploy and local stack scripts.
- Use `tooling/` for dev, CI, and release automation.

## Migration note

Legacy root paths remain available while the repo transitions to the new structure. New work should prefer the canonical domains listed above.

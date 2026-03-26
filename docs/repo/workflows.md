# Repo workflows

Use the workflow that matches the ownership boundary of the change.

## Engine change workflow

Use this for reusable runtime, model, adapter, core, and binding changes.

1. Make the implementation under `engine/`.
2. Keep the public runtime namespace under `nautilus_trader.*` unless there is an explicit migration plan.
3. Update engine-facing docs when public behavior or extension points change.
4. Do not pull strategy- or deployment-specific behavior into the engine to avoid convenience coupling.

## System change workflow

Use this for strategy/runtime systems such as Flux.

1. Implement the change under `systems/<system>/`.
2. Use the system namespace directly, for example `flux.*`, and keep product/deploy naming aligned with the same system name.
3. Keep the naming split explicit: product/deploy surfaces use `flux`, while engine imports remain `nautilus_trader.*`.
4. If a legacy import path exists, update the compatibility layer instead of adding new implementation code under the legacy tree.
5. Update the system README and system-specific docs when structure or runtime contracts change.

## App change workflow

Use this for dashboards, panels, operator UX, and browser-delivered clients.

1. Keep the implementation under `apps/<app>/`.
2. Treat apps as consumers of system and engine APIs, not owners of them.
3. Put app runbooks, local development notes, and test instructions with the app or its app-specific docs.
4. Do not place production backend orchestration logic in the app tree.

## Ops change workflow

Use this for deployment and live-run concerns.

1. Put systemd units, env templates, service wrappers, and operational runbooks in `ops/`.
2. Keep operational defaults and environment-specific wiring out of `engine/`.
3. Reference canonical system entrypoints rather than duplicating runtime logic in shell scripts.
4. Update runbooks whenever service names, env contracts, or launch sequences change.
5. Treat deploy lanes as first-class architecture: `dev` is mutable, while `pilot` and `prod` must run from pinned release roots only.
6. Do not let live services resolve to repo checkouts or worktrees; lane env files must point only at immutable release roots.

## Tooling change workflow

Use this for developer, CI, and release automation.

1. Put local developer helpers in `tooling/dev/`.
2. Put CI entrypoints and validation wrappers in `tooling/ci/`.
3. Put packaging, publishing, and distribution automation in `tooling/release/`.
4. Keep deployment automation out of `tooling/`; it belongs in `ops/`.
5. If a legacy `scripts/` path still exists, update the canonical script first and keep the legacy path as compatibility only.
6. Keep `tooling/ci/check-repo-structure.sh` aligned with the canonical ownership model whenever repo paths change.

## Research workflow

Use this for prototypes, experiments, notebooks, and throwaway analysis.

1. Keep exploratory work under `research/`.
2. Promote code into `engine/`, `systems/`, or `apps/` only when it becomes production-owned.
3. Avoid making production code depend on artifacts from `research/`.
4. If research proves out a reusable primitive, move that primitive into its owning domain and leave a thin example behind.

## Cross-cutting change workflow

Use this when a change spans engine, system, app, ops, and tooling boundaries.

1. Start from the owning runtime boundary, not the UI or shell wrapper.
2. Keep shared contracts single-sourced.
3. Update every affected documentation layer: repo docs, system docs, app docs, runbooks, and tooling references as needed.
4. Prefer temporary compatibility shims over large flag-day renames.
5. Plan removal of compatibility layers once downstream consumers are migrated.

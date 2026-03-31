# Strategy Platform Hardening Wave PR1 Shared Strategy Foundations Implementation Plan

> **For the execution agent:** REQUIRED SUB-SKILL: Before implementing this plan, choose exactly one execution mode and use the matching skill: `superpowers:subagent-driven-development` for same-session execution or `superpowers:executing-plans` for a separate-session handoff.
> **Progress:** The Progress Tracker in this document is the execution source of truth and must be updated on every state change.

**Goal:** Remove Makerv3-owned contract types, config bases, and param-composition helpers from Makerv4 and equities consumers while establishing a data-only registry boundary and first-class common market identity.

**Architecture:** Create explicit cross-family contract homes for strategy types and shared config mixins, keep runtime-param registries and spec composition in `flux.common.params`, create `flux.common.market_identity`, move `FluxStrategyCapabilities` and neutral strategy-contract identity helpers into `flux.common`, and fix `flux.strategies.__init__` plus `flux.strategies.registry` so strategy identity/spec lookup no longer eagerly imports IBKR-dependent classes. `FluxStrategySpec` must stay data-only. This PR has two internal checkpoints: first the registry/import-boundary plus identity foundations, then the type/config/runtime-param migration onto those foundations.

**Tech Stack:** Python strategy config/runtime code, Flux common contracts, API params/profile contract tests, pytest.

**Context Docs:**
- Design: `docs/plans/2026-03-31-strategy-platform-hardening-wave-design.md`
- PRD: `docs/prd/2026-03-31-strategy-platform-hardening-wave.md`
- Relevant specs/runbooks: `docs/plans/2026-03-17-equities-makerv4-split-design.md`, `systems/flux/docs/makerv3.md`

**Decision Summary:**
- runtime param registries remain under `flux.common.params`
- shared config mixins may live under `strategies/shared`, but param schemas/defaults may not
- Makerv4 and equities consumers must stop importing Makerv3-owned contract types in this PR
- this PR explicitly owns the eager import-boundary leak in `flux.strategies.__init__` and `flux.strategies.registry`
- this PR also owns moving `FluxStrategyCapabilities`, typed market identity, and neutral strategy normalization into `flux.common`
- this PR introduces the permanent architecture-boundary test used by later extraction PRs

## Release Gates

| Environment | Required for merge? | Requirement |
| --- | --- | --- |
| `default-unit` | yes | architecture-boundary, common, API, and non-IB registry/import-boundary tests must pass |
| `ibkr-unit` | yes | `python -c "import ibapi"` must succeed and Makerv4/equities proof must pass |
| `pilot` | yes | deploy one pinned pilot release from the PR head and validate the smoke bundle below |

## Affected Pilot Surfaces

- strategy registry and package import surfaces
- `/api/v1/param-schema` and strategy-parameter endpoints
- equities and Makerv4 strategy bootstrap/import paths

## Pilot Stacks To Move Together

- `tokenmm`
- `equities`
- `flux-api` if it is deployed as a separate pilot stack

## Pilot Deploy Units And Promotion Order

Deploy units:

- `flux.api.app`
- `flux.runners.tokenmm.run_node`
- `flux.runners.equities.run_node`

Promotion order:

1. build one pinned pilot release from the exact PR head
2. deploy `flux.api.app`, `flux.runners.tokenmm.run_node`, and `flux.runners.equities.run_node` from that same release
3. validate strategy metadata lookup, parameter-schema endpoints, and Makerv4/equities bootstrap
4. promote the same units together only after the smoke bundle is clean

## Pilot Smoke Bundle

1. Deploy the pilot release from the exact PR head.
2. Confirm strategy metadata and parameter-schema endpoints still resolve all four current strategy families.
3. Confirm Makerv4 and equities stacks still bootstrap cleanly in the pilot environment.
4. Confirm no operator-facing strategy id, `param_set`, or profile-key contract changes are visible.

## PR-Local Rollback

- revert the whole PR
- redeploy the previous pinned pilot release
- do not keep lazy-registry changes while reverting only one family consumer

## Progress Tracker

**Source of truth:** Update this table whenever task state changes. Do not rely on memory, chat history, or TodoWrite alone.

| Task | Status | Owner | Depends On | Write Scope | Lane Branch | Worktree Path | Commit / Diff | Verification | Notes / Last Update |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Overall | not_started | unassigned | none | `systems/flux/flux/common`, `systems/flux/flux/strategies/shared`, `systems/flux/flux/strategies/makerv3`, `systems/flux/flux/strategies/makerv4`, `systems/flux/flux/strategies/equities_maker`, `systems/flux/flux/strategies/equities_taker`, `systems/flux/flux/api`, `tests/unit_tests/flux` | `wave/pr1-shared-foundations` | `.worktrees/strategy-platform-pr1` | none | not_run | Plan created |
| Task 1: Lock import-boundary, architecture-guard, and contract tests | not_started | unassigned | none | `tests/unit_tests/flux/test_architecture_boundaries.py`, `tests/unit_tests/flux/common/test_market_identity.py`, `tests/unit_tests/flux/common/test_strategy_contracts.py`, `tests/unit_tests/flux/common/test_strategy_capabilities.py`, `tests/unit_tests/flux/strategies/equities_maker/test_strategy.py`, `tests/unit_tests/flux/strategies/equities_taker/test_strategy.py`, `tests/unit_tests/flux/strategies/makerv4/test_runtime_params.py`, `tests/unit_tests/flux/strategies/makerv4/test_identity_map.py`, `tests/unit_tests/flux/strategies/makerv4/test_instruments.py`, `tests/unit_tests/flux/strategies/makerv4/test_observability_and_exports.py`, `tests/unit_tests/flux/api/test_app.py`, `tests/unit_tests/flux/api/test_equities_profile_contract.py`, `tests/unit_tests/flux/api/test_param_schema_snapshots.py` | `wave/pr1-shared-foundations` | `.worktrees/strategy-platform-pr1` | none | not_run | Plan created |
| Task 2: Build data-only registry boundary and common identity foundations | not_started | unassigned | Task 1: Lock import-boundary, architecture-guard, and contract tests | `systems/flux/flux/common/market_identity.py`, `systems/flux/flux/common/strategy_types.py`, `systems/flux/flux/common/strategy_capabilities.py`, `systems/flux/flux/common/strategy_contracts.py`, `systems/flux/flux/strategies/shared/config.py`, `systems/flux/flux/strategies/shared/capabilities.py`, `systems/flux/flux/strategies/__init__.py`, `systems/flux/flux/strategies/registry.py`, `tests/unit_tests/flux/test_architecture_boundaries.py`, `tests/unit_tests/flux/common/test_market_identity.py`, `tests/unit_tests/flux/common/test_strategy_contracts.py`, `tests/unit_tests/flux/common/test_strategy_capabilities.py` | `wave/pr1-shared-foundations` | `.worktrees/strategy-platform-pr1` | none | not_run | Checkpoint A |
| Task 3: Migrate shared types, config bases, and runtime-param composition onto the new foundations | not_started | unassigned | Task 2: Build data-only registry boundary and common identity foundations | `systems/flux/flux/common/params.py`, `systems/flux/flux/strategies/makerv3/strategy.py`, `systems/flux/flux/strategies/makerv4/strategy.py`, `systems/flux/flux/strategies/equities_maker/strategy.py`, `systems/flux/flux/strategies/equities_taker/strategy.py`, `systems/flux/flux/strategies/equities_maker/runtime_params.py`, `systems/flux/flux/strategies/equities_taker/runtime_params.py`, `systems/flux/flux/strategies/makerv4/runtime_params.py`, `tests/unit_tests/flux/common/test_params.py`, `tests/unit_tests/flux/strategies/equities_maker/test_runtime_params.py`, `tests/unit_tests/flux/strategies/equities_taker/test_runtime_params.py`, `tests/unit_tests/flux/strategies/equities_maker/test_strategy.py`, `tests/unit_tests/flux/strategies/equities_taker/test_strategy.py`, `tests/unit_tests/flux/strategies/makerv4/test_runtime_params.py`, `tests/unit_tests/flux/strategies/makerv4/test_identity_map.py`, `tests/unit_tests/flux/strategies/makerv4/test_instruments.py` | `wave/pr1-shared-foundations` | `.worktrees/strategy-platform-pr1` | none | not_run | Checkpoint B |
| Task 4: Verify consumer migration, import removal, and rollback note | not_started | unassigned | Task 3: Migrate shared types, config bases, and runtime-param composition onto the new foundations | `systems/flux/flux/api/app.py`, `systems/flux/flux/api/_payloads_common.py`, `tests/unit_tests/flux/test_architecture_boundaries.py`, `docs/plans/2026-03-31-strategy-platform-hardening-wave-pr1-shared-strategy-foundations.md` | `wave/pr1-shared-foundations` | `.worktrees/strategy-platform-pr1` | none | not_run | Plan created |

## Required Internal Checkpoints

Checkpoint A:

- Task 1 and Task 2 complete
- lazy registry/import-boundary fixed
- `FluxStrategySpec` proven data-only
- common market identity and architecture-boundary test are in place

Checkpoint B:

- Task 3 and Task 4 complete
- family config/runtime-param consumers migrated
- public API/schema contracts still match frozen surfaces

---

### Task 1: Lock import-boundary, architecture-guard, and contract tests

**Files:**
- Create: `tests/unit_tests/flux/test_architecture_boundaries.py`
- Create: `tests/unit_tests/flux/common/test_market_identity.py`
- Create: `tests/unit_tests/flux/common/test_strategy_contracts.py`
- Create: `tests/unit_tests/flux/common/test_strategy_capabilities.py`
- Modify: `tests/unit_tests/flux/strategies/equities_maker/test_strategy.py`
- Modify: `tests/unit_tests/flux/strategies/equities_taker/test_strategy.py`
- Modify: `tests/unit_tests/flux/strategies/makerv4/test_runtime_params.py`
- Modify: `tests/unit_tests/flux/strategies/makerv4/test_identity_map.py`
- Modify: `tests/unit_tests/flux/strategies/makerv4/test_instruments.py`
- Modify: `tests/unit_tests/flux/strategies/makerv4/test_observability_and_exports.py`
- Modify: `tests/unit_tests/flux/api/test_app.py`
- Modify: `tests/unit_tests/flux/api/test_equities_profile_contract.py`
- Create: `tests/unit_tests/flux/api/test_param_schema_snapshots.py`

**Dependencies:** `none`

**Write Scope:** `tests/unit_tests/flux/test_architecture_boundaries.py`, `tests/unit_tests/flux/common/test_market_identity.py`, `tests/unit_tests/flux/common/test_strategy_contracts.py`, `tests/unit_tests/flux/common/test_strategy_capabilities.py`, `tests/unit_tests/flux/strategies/equities_maker/test_strategy.py`, `tests/unit_tests/flux/strategies/equities_taker/test_strategy.py`, `tests/unit_tests/flux/strategies/makerv4/test_runtime_params.py`, `tests/unit_tests/flux/strategies/makerv4/test_identity_map.py`, `tests/unit_tests/flux/strategies/makerv4/test_instruments.py`, `tests/unit_tests/flux/strategies/makerv4/test_observability_and_exports.py`, `tests/unit_tests/flux/api/test_app.py`, `tests/unit_tests/flux/api/test_equities_profile_contract.py`, `tests/unit_tests/flux/api/test_param_schema_snapshots.py`

**Verification Commands:**
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/test_architecture_boundaries.py tests/unit_tests/flux/common/test_market_identity.py tests/unit_tests/flux/common/test_strategy_contracts.py tests/unit_tests/flux/common/test_strategy_capabilities.py tests/unit_tests/flux/api/test_app.py tests/unit_tests/flux/api/test_equities_profile_contract.py tests/unit_tests/flux/api/test_param_schema_snapshots.py -q -k 'architecture or market_identity or param_schema or parameters_endpoint or equities_profile or strategy_contracts or capabilities'`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/strategies/makerv4/test_runtime_params.py tests/unit_tests/flux/strategies/makerv4/test_identity_map.py tests/unit_tests/flux/strategies/makerv4/test_instruments.py tests/unit_tests/flux/strategies/makerv4/test_observability_and_exports.py tests/unit_tests/flux/strategies/equities_maker/test_runtime_params.py tests/unit_tests/flux/strategies/equities_maker/test_strategy.py tests/unit_tests/flux/strategies/equities_taker/test_runtime_params.py tests/unit_tests/flux/strategies/equities_taker/test_strategy.py -q`

**Step 1: Write the failing tests**

Lock:

- the permanent forbidden-import and deleted-path architecture rules for this wave
- typed market identity and data-only registry metadata behavior
- Makerv4/equities strategy configs no longer depending on Makerv3-owned types
- normalized common capability and strategy-contract helpers replacing strategy-local ownership
- strategy metadata lookups no longer depending on eager equities class imports
- API params/profile routing still returning the same external contract, including golden param-schema fixtures

**Step 2: Run tests to verify they fail**

Run the API/common slice in `default-unit` and the Makerv4/equities slice in `ibkr-unit`.

**Step 3: Commit**

```bash
git add tests/unit_tests/flux/test_architecture_boundaries.py \
  tests/unit_tests/flux/common/test_market_identity.py \
  tests/unit_tests/flux/common/test_strategy_contracts.py \
  tests/unit_tests/flux/common/test_strategy_capabilities.py \
  tests/unit_tests/flux/strategies/equities_maker/test_strategy.py \
  tests/unit_tests/flux/strategies/equities_taker/test_strategy.py \
  tests/unit_tests/flux/strategies/makerv4/test_runtime_params.py \
  tests/unit_tests/flux/strategies/makerv4/test_identity_map.py \
  tests/unit_tests/flux/strategies/makerv4/test_instruments.py \
  tests/unit_tests/flux/strategies/makerv4/test_observability_and_exports.py \
  tests/unit_tests/flux/api/test_app.py \
  tests/unit_tests/flux/api/test_equities_profile_contract.py \
  tests/unit_tests/flux/api/test_param_schema_snapshots.py
git commit -m "test: lock shared strategy foundation boundaries"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 2: Build data-only registry boundary and common identity foundations

**Files:**
- Create: `systems/flux/flux/common/market_identity.py`
- Create: `systems/flux/flux/common/strategy_types.py`
- Create: `systems/flux/flux/common/strategy_capabilities.py`
- Create: `systems/flux/flux/strategies/shared/config.py`
- Modify: `systems/flux/flux/common/strategy_contracts.py`
- Delete: `systems/flux/flux/strategies/shared/capabilities.py`
- Modify: `systems/flux/flux/strategies/__init__.py`
- Modify: `systems/flux/flux/strategies/registry.py`
- Modify: `tests/unit_tests/flux/test_architecture_boundaries.py`
- Modify: `tests/unit_tests/flux/common/test_market_identity.py`
- Modify: `tests/unit_tests/flux/common/test_strategy_contracts.py`
- Modify: `tests/unit_tests/flux/common/test_strategy_capabilities.py`

**Dependencies:** `Task 1: Lock import-boundary, architecture-guard, and contract tests`

**Write Scope:** `systems/flux/flux/common/market_identity.py`, `systems/flux/flux/common/strategy_types.py`, `systems/flux/flux/common/strategy_capabilities.py`, `systems/flux/flux/common/strategy_contracts.py`, `systems/flux/flux/strategies/shared/config.py`, `systems/flux/flux/strategies/shared/capabilities.py`, `systems/flux/flux/strategies/__init__.py`, `systems/flux/flux/strategies/registry.py`, `tests/unit_tests/flux/test_architecture_boundaries.py`, `tests/unit_tests/flux/common/test_market_identity.py`, `tests/unit_tests/flux/common/test_strategy_contracts.py`, `tests/unit_tests/flux/common/test_strategy_capabilities.py`

**Verification Commands:**
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/test_architecture_boundaries.py tests/unit_tests/flux/common/test_market_identity.py tests/unit_tests/flux/common/test_strategy_contracts.py tests/unit_tests/flux/common/test_strategy_capabilities.py tests/unit_tests/flux/api/test_app.py tests/unit_tests/flux/api/test_param_schema_snapshots.py -q -k 'architecture or market_identity or param_schema or parameters_endpoint'`

**Step 1: Add the common market-identity and type modules**

Move shared literals such as `OrderQtyUnit` and `SpotCashBorrowingPolicy` into `flux.common.strategy_types` and add the first-class common market-identity surface in `flux.common.market_identity`.

**Step 2: Add shared config mixins**

Create a shared config layer for maker/reference strategy families so `MakerV4StrategyConfig` no longer inherits from `MakerV3StrategyConfig`.

**Step 3: Move capability and identity helpers to common**

Move `FluxStrategyCapabilities` out of `strategies/shared` and tighten `flux.common.strategy_contracts` so later PRs consume one neutral identity/capability surface instead of recreating family-local suffix heuristics.

**Step 4: Fix the lazy registry boundary and keep `FluxStrategySpec` data-only**

Refactor `flux.strategies.__init__` and `flux.strategies.registry` so strategy identity/spec lookup preserves current public behavior without eagerly importing IBKR-dependent strategy classes. The registry metadata table may hold ids, import paths, config paths, and capability metadata, but not imported class objects.

**Step 5: Update the architecture-boundary manifests**

Add or update declarative forbidden-import and deleted-path entries so later PRs fail fast if they reintroduce MakerV3-owned foundations.

**Step 6: Commit**

```bash
git add systems/flux/flux/common/market_identity.py \
  systems/flux/flux/common/strategy_types.py \
  systems/flux/flux/common/strategy_capabilities.py \
  systems/flux/flux/common/strategy_contracts.py \
  systems/flux/flux/strategies/shared/config.py \
  systems/flux/flux/strategies/shared/capabilities.py \
  systems/flux/flux/strategies/__init__.py \
  systems/flux/flux/strategies/registry.py \
  tests/unit_tests/flux/test_architecture_boundaries.py \
  tests/unit_tests/flux/common/test_market_identity.py \
  tests/unit_tests/flux/common/test_strategy_contracts.py \
  tests/unit_tests/flux/common/test_strategy_capabilities.py
git commit -m "refactor: add common identity foundations and lazy registry boundary"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 3: Migrate shared types, config bases, and runtime-param composition onto the new foundations

**Files:**
- Modify: `systems/flux/flux/common/params.py`
- Modify: `systems/flux/flux/strategies/makerv3/strategy.py`
- Modify: `systems/flux/flux/strategies/makerv4/strategy.py`
- Modify: `systems/flux/flux/strategies/equities_maker/strategy.py`
- Modify: `systems/flux/flux/strategies/equities_taker/strategy.py`
- Modify: `systems/flux/flux/strategies/equities_maker/runtime_params.py`
- Modify: `systems/flux/flux/strategies/equities_taker/runtime_params.py`
- Modify: `systems/flux/flux/strategies/makerv4/runtime_params.py`
- Modify: `tests/unit_tests/flux/common/test_params.py`
- Modify: `tests/unit_tests/flux/strategies/equities_maker/test_runtime_params.py`
- Modify: `tests/unit_tests/flux/strategies/equities_taker/test_runtime_params.py`
- Modify: `tests/unit_tests/flux/strategies/equities_maker/test_strategy.py`
- Modify: `tests/unit_tests/flux/strategies/equities_taker/test_strategy.py`
- Modify: `tests/unit_tests/flux/strategies/makerv4/test_runtime_params.py`
- Modify: `tests/unit_tests/flux/strategies/makerv4/test_identity_map.py`
- Modify: `tests/unit_tests/flux/strategies/makerv4/test_instruments.py`

**Dependencies:** `Task 2: Build data-only registry boundary and common identity foundations`

**Write Scope:** `systems/flux/flux/common/params.py`, `systems/flux/flux/strategies/makerv3/strategy.py`, `systems/flux/flux/strategies/makerv4/strategy.py`, `systems/flux/flux/strategies/equities_maker/strategy.py`, `systems/flux/flux/strategies/equities_taker/strategy.py`, `systems/flux/flux/strategies/equities_maker/runtime_params.py`, `systems/flux/flux/strategies/equities_taker/runtime_params.py`, `systems/flux/flux/strategies/makerv4/runtime_params.py`, `tests/unit_tests/flux/common/test_params.py`, `tests/unit_tests/flux/strategies/equities_maker/test_runtime_params.py`, `tests/unit_tests/flux/strategies/equities_taker/test_runtime_params.py`, `tests/unit_tests/flux/strategies/equities_maker/test_strategy.py`, `tests/unit_tests/flux/strategies/equities_taker/test_strategy.py`, `tests/unit_tests/flux/strategies/makerv4/test_runtime_params.py`, `tests/unit_tests/flux/strategies/makerv4/test_identity_map.py`, `tests/unit_tests/flux/strategies/makerv4/test_instruments.py`

**Verification Commands:**
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/common/test_params.py tests/unit_tests/flux/common/test_market_identity.py tests/unit_tests/flux/common/test_strategy_contracts.py tests/unit_tests/flux/common/test_strategy_capabilities.py tests/unit_tests/flux/strategies/equities_maker/test_runtime_params.py tests/unit_tests/flux/strategies/equities_taker/test_runtime_params.py tests/unit_tests/flux/strategies/equities_maker/test_strategy.py tests/unit_tests/flux/strategies/equities_taker/test_strategy.py tests/unit_tests/flux/strategies/makerv4/test_runtime_params.py tests/unit_tests/flux/strategies/makerv4/test_identity_map.py tests/unit_tests/flux/strategies/makerv4/test_instruments.py -q`

**Step 1: Migrate family consumers to the new common/shared homes**

Update Makerv3, Makerv4, and equities family configs and strategy modules to import shared types, config mixins, and common identity helpers from the new owners.

**Step 2: Add repo-wide spec-composition helpers**

Move Makerv3-derived spec-cloning logic out of strategy-local runtime-param modules and into `flux.common.params`.

**Step 3: Update runtime-param modules**

Make equities and Makerv4 runtime-param modules consume the repo-wide helper rather than cloning Makerv3 internals locally.

**Step 4: Re-run focused tests**

Run the common slice in `default-unit` and the family runtime-param slice in `ibkr-unit`. The external contract must remain preserved.

**Step 5: Commit**

```bash
git add systems/flux/flux/common/params.py \
  systems/flux/flux/strategies/makerv3/strategy.py \
  systems/flux/flux/strategies/makerv4/strategy.py \
  systems/flux/flux/strategies/equities_maker/strategy.py \
  systems/flux/flux/strategies/equities_taker/strategy.py \
  systems/flux/flux/strategies/equities_maker/runtime_params.py \
  systems/flux/flux/strategies/equities_taker/runtime_params.py \
  systems/flux/flux/strategies/makerv4/runtime_params.py \
  tests/unit_tests/flux/common/test_params.py \
  tests/unit_tests/flux/strategies/equities_maker/test_runtime_params.py \
  tests/unit_tests/flux/strategies/equities_taker/test_runtime_params.py \
  tests/unit_tests/flux/strategies/equities_maker/test_strategy.py \
  tests/unit_tests/flux/strategies/equities_taker/test_strategy.py \
  tests/unit_tests/flux/strategies/makerv4/test_runtime_params.py \
  tests/unit_tests/flux/strategies/makerv4/test_identity_map.py \
  tests/unit_tests/flux/strategies/makerv4/test_instruments.py
git commit -m "refactor: keep runtime param composition repo wide"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 4: Verify consumer migration, import removal, and rollback note

**Files:**
- Modify: `systems/flux/flux/api/app.py`
- Modify: `systems/flux/flux/api/_payloads_common.py`
- Modify: `tests/unit_tests/flux/test_architecture_boundaries.py`
- Modify: `docs/plans/2026-03-31-strategy-platform-hardening-wave-pr1-shared-strategy-foundations.md`

**Dependencies:** `Task 3: Migrate shared types, config bases, and runtime-param composition onto the new foundations`

**Write Scope:** `systems/flux/flux/api/app.py`, `systems/flux/flux/api/_payloads_common.py`, `tests/unit_tests/flux/test_architecture_boundaries.py`, `docs/plans/2026-03-31-strategy-platform-hardening-wave-pr1-shared-strategy-foundations.md`

**Verification Commands:**
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/test_architecture_boundaries.py tests/unit_tests/flux/api/test_app.py tests/unit_tests/flux/api/test_equities_profile_contract.py tests/unit_tests/flux/api/test_param_schema_snapshots.py -q -k 'architecture or param_schema or parameters_endpoint or equities_profile'`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/strategies/makerv4/test_identity_map.py tests/unit_tests/flux/strategies/makerv4/test_instruments.py tests/unit_tests/flux/strategies/makerv4/test_observability_and_exports.py tests/unit_tests/flux/strategies/equities_maker/test_strategy.py tests/unit_tests/flux/strategies/equities_taker/test_strategy.py -q`
- `rg -n "from flux\\.strategies\\.makerv3|import .*makerv3" systems/flux/flux/strategies/{makerv4,equities_maker,equities_taker}`

**Step 1: Migrate any remaining API or metadata consumers**

Preserve the external API contract while switching to the new common/shared foundations. This includes any remaining metadata lookups using `flux.strategies.registry`.

**Step 2: Verify import removal and architecture manifests**

The ripgrep command must return no production-code matches for Makerv3-owned contract imports in Makerv4 and equities families, and the architecture-boundary test must encode the same rule permanently.

**Step 3: Record rollback note**

Document that rollback is revert-safe because the PR changes internal ownership, not public names.

**Step 4: Commit**

```bash
git add systems/flux/flux/api/app.py \
  systems/flux/flux/api/_payloads_common.py \
  tests/unit_tests/flux/test_architecture_boundaries.py \
  docs/plans/2026-03-31-strategy-platform-hardening-wave-pr1-shared-strategy-foundations.md
git commit -m "docs: record pr1 verification and rollback"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

# Strategy Platform Hardening Wave Review Packet

**Date:** 2026-03-31

Canonical review set note: only documents named `2026-03-31-strategy-platform-hardening-wave*.md` are part of the active review packet for this wave. Older March 31 `wave-*` drafts are superseded scratch material and are not part of the canonical stack.

## Executive Summary

This packet describes a repo-wide strategy-platform hardening wave for:

- `makerv3`
- `makerv4`
- `equities_maker`
- `equities_taker`
- the shared/common/registry layers they should depend on

The wave has two equal goals:

1. Clean up and harden the current live strategy surfaces for production.
2. Leave behind a real shared strategy platform so future shared improvements land once without another MakerV3-first extraction cycle.

Execution rules:

- no compatibility shims
- every PR independently mergeable and releasable
- revert-only rollback at PR granularity
- behavior-preserving except documented bug fixes and contract corrections

This packet also assumes four explicit execution safeguards:

1. an executable architecture-boundary test with forbidden-import and deleted-path manifests
2. a data-only `FluxStrategySpec` and typed `flux.common.market_identity` contract in `PR1`
3. golden fixtures for representative frozen payload and schema surfaces
4. child-plan-level deploy-unit and promotion-order rules for each runtime-code PR

## Why This Wave Exists

Current `main` still has inverted ownership:

- `makerv4` imports MakerV3-owned inventory, managed-order, publisher, constants, and config surfaces
- `equities_maker` and `equities_taker` import MakerV3-owned strategy types
- `shared/*` still imports MakerV3 publisher helpers
- `flux.strategies.__init__` and `flux.strategies.registry` eagerly import equities strategy classes and leak optional IBKR dependencies
- important blocked-state, alert, balances, and realtime contracts are real operational surfaces but are not governed as one frozen set

That makes MakerV3 the accidental platform owner for newer strategy families and keeps future multi-market work coupled to legacy family-local abstractions.

## Verified Current Facts

These were verified against current `main` on 2026-03-31:

1. `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/strategies/makerv3 -q`
   - result: `1 failed, 289 passed`
   - failure: borrow-cap rejection contract mismatch
2. `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/strategies/makerv4 tests/unit_tests/flux/strategies/equities_maker tests/unit_tests/flux/strategies/equities_taker tests/unit_tests/flux/strategies/shared -q`
   - result: collection blocked in this environment because `ibapi` is missing
3. Concrete dependency edges still exist from `makerv4` and `shared/*` into MakerV3-owned reusable modules.

## Defined Term: `PLUME-shaped`

`PLUME-shaped` means a supposedly shared or common abstraction still assumes one concrete symbol family, venue pair, or instrument-id suffix pattern is the canonical case.

Examples of PLUME-shaped failure modes:

- suffix parsing baked into shared/common helpers
- strategy-generic logic that assumes one spot/perp naming convention
- venue-policy code that only really works for one family of rejection strings

## Current vs Target Dependency Graph

### Current

```text
makerv4 ----------> makerv3.inventory
makerv4 ----------> makerv3.managed_orders
makerv4 ----------> makerv3.publisher
makerv4 ----------> makerv3.constants
makerv4 ----------> MakerV3StrategyConfig

equities_taker ---> makerv3.strategy.OrderQtyUnit
equities_taker ---> makerv3.strategy.SpotCashBorrowingPolicy
equities_maker ---> makerv3.strategy.OrderQtyUnit
equities_maker ---> makerv3.strategy.SpotCashBorrowingPolicy

shared.alerts ----> makerv3.publisher
shared.equities_arb.hedging -> makerv3.publisher

flux.strategies.__init__/registry -> eager equities strategy imports
```

### Target

```text
makerv3 ---------> flux.common
makerv3 ---------> flux.strategies.shared
makerv3 ---------> flux.strategies.registry

makerv4 ---------> flux.common
makerv4 ---------> flux.strategies.shared
makerv4 ---------> flux.strategies.registry

equities_taker --> flux.common
equities_taker --> flux.strategies.shared
equities_taker --> flux.strategies.registry

equities_maker --> flux.common
equities_maker --> flux.strategies.shared
equities_maker --> flux.strategies.registry

flux.strategies.shared -> flux.common
flux.strategies.registry -> constant metadata + lazy class loading

flux.runners.shared ---> flux.common
flux.runners.shared ---> flux.strategies.registry
```

Strategies do not depend on `flux.runners.shared` in the target state.

## Boundary Design Summary

### `flux.common`

Owns repo-wide generic primitives:

- runtime-param registry composition helpers
- quantity-unit normalization and exposure conversion primitives
- strategy capability models and normalized strategy-contract identity helpers
- account-projection read-model helpers
- neutral identity/capability helpers introduced by the wave

### `flux.strategies.shared`

Owns reusable strategy-runtime primitives:

- shared config adapters/mixins
- shared observability helpers
- legacy topic constants and payload-schema helpers for already-frozen external contracts under an explicit `observability/contracts` namespace
- managed-order primitives
- strategy-facing inventory adapters built on `flux.common`
- shared venue-policy parsing using normalized inputs

### `flux.strategies.registry`

Owns strategy identity and lookup:

- `FluxStrategyIdentity`
- `FluxStrategySpec`
- lazy class/config loading to remove eager optional-dependency leaks

`FluxStrategySpec` is intentionally data-only: strategy id, `param_set`, config type path, class import path, capabilities, and deploy-surface metadata. The metadata table must not embed imported strategy class objects.

### `flux.runners.shared`

Owns runner services only, including the runner-owned IBKR reference-balance provider. It may depend on `flux.common` and `flux.strategies.registry`, but strategies must not depend on runners.

### Family-local ownership stays local

- MakerV3 quote orchestration and state machine behavior
- MakerV4 hedge/control behavior
- equities-family trading semantics
- family-specific payload assembly and publication timing

Important nuance: topic constants for frozen legacy contracts may move to shared observability ownership, but family-specific payload assembly and publish timing still stay local unless explicitly extracted.

## Goals

### Production goals

1. Make the current strategy surfaces safer to modify and review.
2. Reduce MakerV3 hot-path and orchestration complexity.
3. Freeze operator-facing state/event/alert/balances contracts tightly enough for straight-to-prod no-shim work.
4. Increase direct invariant-style coverage for shared/common platform surfaces.

### Platform goals

1. Stop using MakerV3 as the de facto shared platform.
2. Establish durable shared ownership across `flux.common`, `flux.strategies.shared`, and `flux.strategies.registry`.
3. Remove PLUME-shaped ownership traps from the migrated shared/common surfaces.
4. Make future shared strategy improvements land once and fan out cleanly.

## Non-Goals

This wave is not trying to:

1. rename legacy topic namespaces away from `flux.makerv3.*`
2. build a standalone strategy platform service
3. add new strategy families or venues
4. smuggle new trading behavior into structural PRs

## Operator Contract Freeze

The wave treats the following as frozen unless a child PR documents a bug fix or additive-only change:

| Surface | Producer | Consumers | Freeze rule |
| --- | --- | --- | --- |
| `flux.makerv3.state` | strategy publisher | API signals, readiness, bridge, UI | preserve topic and field semantics |
| `flux.makerv3.event` | strategy publisher | persistence, review, analytics flows | preserve envelope semantics |
| `flux.makerv3.alert` | strategy publisher | alerts API and realtime | preserve routing and actionable semantics |
| `flux.makerv3.market_bbo` | strategy publisher | monitoring and debugging | preserve routing and payload meaning |
| `flux.makerv3.fv` | strategy publisher | monitoring and debugging | preserve routing and payload meaning |
| `flux.makerv3.balances` | strategy publisher | balances API, realtime, portfolio views | preserve payload shape |
| `flux.makerv3.order_intent` | strategy publisher | order-action and fill enrichment | preserve linkage semantics |
| `flux.makerv3.trade` | strategy publisher | downstream monitoring | preserve identity and quantity semantics |
| blocked-state names | strategy state | readiness and operator consumers | preserve names and meaning unless bug fix |
| `quote_progress`, `quote_blockers`, `quote_health`, pending-cancel diagnostics | strategy state payload | API, readiness, operators | preserve meaning unless explicitly revised and frozen |
| `/api/v1/signals` | API | UI and operator tooling | preserve field semantics |
| `/api/v1/balances` | API | operator tooling | preserve payload shape and source-of-truth semantics |
| `/api/v1/param-schema` and strategy parameter endpoints | API | operator tooling and config UIs | preserve routing and schema identity |
| bridge suffix topic handling | runner/bridge layer | downstream suffix consumers | preserve routing behavior |

## Release-Blocking Environments

| Environment | Required for | Rule |
| --- | --- | --- |
| `default-unit` | every PR | focused unit slices with `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1` |
| `ibkr-unit` | `PR1` through `PR4` | `python -c "import ibapi"` must succeed; missing `ibapi` is a merge blocker, not a waiver |
| `pilot` | every runtime-code PR | build one pinned pilot release from the PR head and validate the PR-specific smoke bundle; `PR7` is docs/docstrings/tests only and is exempt unless it stops being non-runtime |

## PR Wave Summary

| PR | Purpose | Affected pilot surfaces | Main proof |
| --- | --- | --- | --- |
| `PR0` | fix current borrow-cap mismatch and freeze the blocker/alert/readiness contract | tokenmm Makerv3 stack, signals API, readiness/bridge consumers | MakerV3 suite green plus signals/readiness contract slices |
| `PR1` | move shared type/config foundations out of MakerV3, move capability/identity helpers into `flux.common`, introduce typed `market_identity`, keep `FluxStrategySpec` data-only, and fix eager registry imports | API param/profile surfaces, registry/spec lookups, Makerv4/equities import paths | architecture-boundary proof plus default-unit and ibkr-unit proof that strategy specs resolve without eager optional-dep leakage and no longer depend on `strategies.shared.capabilities` |
| `PR2` | move account-projection lookup into `flux.common` and move runner-owned IBKR reference-balance support into `flux.runners.shared` | profile-account consumers in Makerv3/Makerv4, runner profile-account surfaces, API inventory/balances views | common/read-model tests plus runner/API slices plus Makerv3/Makerv4 inventory/reference-balance proof |
| `PR3` | move shared observability helpers and frozen legacy topic/schema contracts into explicit observability-contract ownership | Makerv3/Makerv4/equities observability, readiness/bridge consumers, API realtime/payload surfaces | architecture-boundary proof plus shared tests, golden payload fixtures, strategy observability, runner bridge/readiness, and API realtime/payload tests |
| `PR4` | move managed-order primitives and strategy-facing inventory adapters out of MakerV3 and consume typed common market identity | Makerv3/Makerv4 order execution and inventory projections | architecture-boundary proof plus shared tests, golden inventory fixtures, order-safety, reconciliation, inventory, common market-identity normalization, and API inventory contract slices |
| `PR5` | split Makerv3 quote pipeline | tokenmm quote lifecycle and telemetry | focused quote-engine, order-intent, and observability slices |
| `PR6` | decompose Makerv3 strategy god object | tokenmm lifecycle, runtime-param, and state-export surfaces | lifecycle, reconciliation, runtime-param, and observability slices |
| `PR7` | residual docs, docstrings, and cleanup | docs, Python docstrings, and residual invariant tests only; no runtime behavior changes allowed | docs tests, residual invariants, and docstring lint; not a whole-wave rerun |

## PR-Specific Notes Worth Reviewing

### `PR0` bug-fix contract

This is the only early behavior-correction PR. The frozen contract is:

- a spot borrow-cap rejection blocks only the affected ask or `SELL` side
- only affected ask managed orders are cancelled
- overall strategy state remains `running`
- `bot_on` remains `true`
- state and API payloads record a side-local `spot_borrow_cap` blocker
- one actionable alert plus structured event is emitted under cooldown gating

The PR does not get to “choose later” between multiple contracts. It implements this one.

### `PR1` import-boundary fix

`PR1` must explicitly modify:

- `systems/flux/flux/strategies/__init__.py`
- `systems/flux/flux/strategies/registry.py`
- `systems/flux/flux/common/market_identity.py` or its equivalent common home
- `systems/flux/flux/common/strategy_capabilities.py` or its equivalent common home
- `systems/flux/flux/common/strategy_contracts.py` for neutral identity/capability normalization used by later PRs

The goal is lazy strategy resolution with the same public lookup behavior, not a new registry surface. `PR1` also has two required internal checkpoints:

1. lazy registry/import-boundary cleanup plus data-only identity/capability/market-identity foundations
2. type/config/runtime-param migration onto those foundations

## Minimum Verification Matrix

| PR | Minimum proof |
| --- | --- |
| `PR0` | `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/strategies/makerv3 -q`; `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/api/test_payloads.py tests/unit_tests/flux/api/test_signals_inventory_contract.py tests/unit_tests/flux/runners/test_tokenmm_readiness.py tests/unit_tests/flux/bridge/test_handlers.py tests/unit_tests/flux/bridge/test_stream_consumer.py -q` |
| `PR1` | `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/test_architecture_boundaries.py tests/unit_tests/flux/common/test_params.py tests/unit_tests/flux/common/test_market_identity.py tests/unit_tests/flux/common/test_strategy_contracts.py tests/unit_tests/flux/common/test_strategy_capabilities.py tests/unit_tests/flux/api/test_app.py tests/unit_tests/flux/api/test_equities_profile_contract.py tests/unit_tests/flux/api/test_param_schema_snapshots.py -q`; in `ibkr-unit`: `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/strategies/makerv4/test_runtime_params.py tests/unit_tests/flux/strategies/makerv4/test_identity_map.py tests/unit_tests/flux/strategies/makerv4/test_instruments.py tests/unit_tests/flux/strategies/makerv4/test_observability_and_exports.py tests/unit_tests/flux/strategies/equities_maker/test_runtime_params.py tests/unit_tests/flux/strategies/equities_maker/test_strategy.py tests/unit_tests/flux/strategies/equities_taker/test_runtime_params.py tests/unit_tests/flux/strategies/equities_taker/test_strategy.py -q` |
| `PR2` | `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/test_architecture_boundaries.py tests/unit_tests/flux/common/test_account_projection_positions.py tests/unit_tests/flux/strategies/makerv3/test_inventory.py tests/unit_tests/flux/runners/shared/test_profile_accounts.py tests/unit_tests/flux/api/test_signals_inventory_contract.py tests/unit_tests/flux/api/test_payloads.py -q`; in `ibkr-unit`: `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/strategies/makerv4/test_inventory_contract.py tests/unit_tests/flux/strategies/makerv4/test_reference_balances.py tests/unit_tests/flux/runners/shared/test_reference_balances.py -q` |
| `PR3` | `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/test_architecture_boundaries.py tests/unit_tests/flux/strategies/shared tests/unit_tests/flux/strategies/shared/test_equities_arb_core.py tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py tests/unit_tests/flux/api/test_payloads.py tests/unit_tests/flux/api/test_payload_snapshots.py tests/unit_tests/flux/api/test_realtime_contract.py tests/unit_tests/flux/runners/test_tokenmm_readiness.py tests/unit_tests/examples/strategies/test_tokenmm_run_bridge.py tests/unit_tests/examples/strategies/test_equities_run_bridge.py -q`; in `ibkr-unit`: `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/strategies/makerv4/test_observability_and_exports.py tests/unit_tests/flux/strategies/makerv4/test_publisher_contract.py -q` |
| `PR4` | `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/test_architecture_boundaries.py tests/unit_tests/flux/common/test_market_identity.py tests/unit_tests/flux/common/test_strategy_contracts.py tests/unit_tests/flux/strategies/shared/test_managed_orders.py tests/unit_tests/flux/strategies/shared/test_inventory_math.py tests/unit_tests/flux/strategies/makerv3/test_managed_orders.py tests/unit_tests/flux/strategies/makerv3/test_inventory.py tests/unit_tests/flux/strategies/makerv3/test_order_safety.py tests/unit_tests/flux/strategies/makerv3/test_reconciliation.py tests/unit_tests/flux/api/test_signals_inventory_contract.py tests/unit_tests/flux/api/test_payload_snapshots.py -q`; in `ibkr-unit`: `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/strategies/makerv4/test_inventory_contract.py tests/unit_tests/flux/strategies/makerv4/test_strategy.py tests/unit_tests/flux/strategies/makerv4/test_instruments.py -q` |
| `PR5` | `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py tests/unit_tests/flux/strategies/makerv3/test_order_intent_exports.py tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py -q` |
| `PR6` | `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/strategies/makerv3/test_strategy_lifecycle.py tests/unit_tests/flux/strategies/makerv3/test_reconciliation.py tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py tests/unit_tests/flux/strategies/makerv3/test_runtime_params.py -q` |
| `PR7` | `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/docs tests/unit_tests/flux/strategies/shared tests/unit_tests/flux/strategies/makerv3 -q`; `ruff check --select D systems/flux/flux/common systems/flux/flux/strategies/shared systems/flux/flux/strategies/makerv3` |

## Pilot Validation And Rollback Philosophy

For every runtime-code PR:

1. build one pinned pilot release from the PR head
2. move the exact child-plan deploy units together from that same release
3. follow the child-plan promotion order so unsupported mixed-version states are never entered
4. validate the PR-specific smoke bundle for the affected stack
5. promote only after pilot is clean

Rollback rule:

- revert the entire PR
- redeploy the previous pinned release
- do not keep the new shared/common ownership while reverting only one consumer

## Multi-Market Acceptance Criteria

This wave only counts as platform generalization if all of the following are true:

1. No production shared/common code hardcodes `PLUME`.
2. Shared/common ownership introduced by the wave does not rely on raw suffix parsing where explicit identity or capability inputs are available.
3. `PR0` proves the borrow-cap path is not PLUME-shaped by test fixtures.
4. `PR1` through `PR4` consume neutral common/shared inputs instead of recreating family-local ownership traps.
5. Future new markets can use shared/common modules without importing MakerV3-owned primitives.

## Key Risks And Mitigations

### Risk 1: hidden operator contract drift

Mitigation:

- explicit contract-freeze table
- API/readiness/realtime verification included in the wave matrix
- golden payload/schema fixtures for representative surfaces

### Risk 2: no-shim rollback is weaker than it looks

Mitigation:

- whole-PR revert rule
- pinned pilot release validation per PR
- explicit deploy-unit and promotion-order rules in child plans

### Risk 3: shared becomes a dumping ground

Mitigation:

- explicit separation across `flux.common`, `flux.strategies.shared`, and `flux.strategies.registry`
- family-specific payload assembly and business logic stay local

### Risk 4: multi-market claims are fake

Mitigation:

- explicit anti-hardcoding criteria
- narrow `PR0` venue-policy tests
- first-class common market identity in `PR1`
- no dedicated “trust us, we generalized it” PR with fuzzy ownership

## Confidence Assessment

Current planning confidence: `0.93`

Reasons:

- coupling and migration edges were verified against current `main`
- the wave now has an explicit operator freeze, environment model, PR ownership model, and rollback rule
- `PR1` now explicitly owns the eager import-boundary fix
- the PR stack is internally consistent at `PR0` through `PR7`

Remaining medium-confidence items:

1. Exact helper filenames inside `PR3` and `PR4` may shift slightly during implementation.
2. The exact split between `PR5` and `PR6` Makerv3-local collaborator files may adjust a little during implementation.

Neither item changes the wave shape.

## Review Questions

Please review this packet with these questions in mind:

1. Are the `flux.common`, `flux.strategies.shared`, `flux.strategies.registry`, and `flux.runners.shared` boundaries correct?
2. Is each PR independently releasable with no shims?
3. Is the rollback model credible for prod?
4. Is the operator contract freeze complete enough?
5. Do the anti-hardcoding criteria actually prevent future PLUME-shaped regressions?

## Linked Detailed Docs

Strategic docs:

- `docs/prd/2026-03-31-strategy-platform-hardening-wave.md`
- `docs/plans/2026-03-31-strategy-platform-hardening-wave-design.md`
- `docs/plans/2026-03-31-strategy-platform-hardening-wave.md`

Child plans:

- `docs/plans/2026-03-31-strategy-platform-hardening-wave-pr0-baseline-safety-and-contract-freeze.md`
- `docs/plans/2026-03-31-strategy-platform-hardening-wave-pr1-shared-strategy-foundations.md`
- `docs/plans/2026-03-31-strategy-platform-hardening-wave-pr2-shared-account-projection-ownership.md`
- `docs/plans/2026-03-31-strategy-platform-hardening-wave-pr3-shared-observability-platform.md`
- `docs/plans/2026-03-31-strategy-platform-hardening-wave-pr4-shared-execution-primitives.md`
- `docs/plans/2026-03-31-strategy-platform-hardening-wave-pr5-makerv3-quote-pipeline-split.md`
- `docs/plans/2026-03-31-strategy-platform-hardening-wave-pr6-makerv3-strategy-decomposition.md`
- `docs/plans/2026-03-31-strategy-platform-hardening-wave-pr7-docstrings-runbooks-and-cleanup.md`

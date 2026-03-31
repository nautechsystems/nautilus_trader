# Strategy Platform Hardening Wave Design

**Date:** 2026-03-31

Canonical review set note: only documents named `2026-03-31-strategy-platform-hardening-wave*.md` are part of the active review packet for this wave. Older March 31 `wave-*` drafts are superseded scratch material and are not part of the canonical stack.

## External Review Context

This design is written to stand on its own.

It assumes post-`#91` `main` and defines one coordinated no-shim PR stack covering:

- MakerV3 production hardening and cleanup
- shared-platform extraction out of MakerV3 ownership
- direct migration of Makerv4, `equities_maker`, `equities_taker`, and `shared/*` consumers
- docs, operator-contract, and test hardening

Mandatory wave constraints:

1. no compatibility shims
2. each PR independently mergeable and releasable
3. straight-to-prod via pinned pilot releases
4. behavior-preserving by default

## Verified Current State

The current repo still has several inverted dependency edges and one concrete red baseline.

### Verified dependency problems

1. `makerv4` still imports MakerV3-owned reusable modules and types:
   - `makerv3.inventory`
   - `makerv3.managed_orders`
   - `makerv3.publisher`
   - MakerV3 topic/constants surfaces
   - `MakerV3StrategyConfig`
2. `equities_maker` and `equities_taker` still import MakerV3-owned strategy types:
   - `OrderQtyUnit`
   - `SpotCashBorrowingPolicy`
3. `shared/*` still imports MakerV3 publisher helpers:
   - `shared.alerts -> makerv3.publisher`
   - `shared.equities_arb.hedging -> makerv3.publisher.decimal_to_json_str`
4. `flux.strategies.__init__` and `flux.strategies.registry` still eagerly import equities strategy classes, which leaks optional IBKR dependencies into unrelated import paths.

### Verified baseline quality problems

1. `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/strategies/makerv3 -q` is red on `main`:
   - `1 failed, 289 passed`
   - failing path: borrow-cap rejection contract mismatch
2. `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/strategies/makerv4 tests/unit_tests/flux/strategies/equities_maker tests/unit_tests/flux/strategies/equities_taker tests/unit_tests/flux/strategies/shared -q` is blocked in this environment because `ibapi` is not installed.

### Defined term: `PLUME-shaped`

`PLUME-shaped` means a shared or common abstraction is secretly designed around one concrete symbol family, venue pair, or instrument-id suffix pattern as the canonical case instead of taking neutral config, manifest, metadata, or capability inputs.

## Before vs After Dependency Graph

### Before

```text
makerv4 -------------------------------> makerv3.inventory / managed_orders / publisher / constants / config
equities_maker ------------------------> makerv3.strategy types
equities_taker ------------------------> makerv3.strategy types
shared.alerts -------------------------> makerv3.publisher
shared.equities_arb.hedging -----------> makerv3.publisher
flux.strategies.__init__/registry -----> eager strategy-class imports -> optional IBKR leak
```

### After

```text
makerv3 -------------------------------> flux.common
makerv3 -------------------------------> flux.strategies.shared
makerv3 -------------------------------> flux.strategies.registry

makerv4 -------------------------------> flux.common
makerv4 -------------------------------> flux.strategies.shared
makerv4 -------------------------------> flux.strategies.registry

equities_maker ------------------------> flux.common
equities_maker ------------------------> flux.strategies.shared
equities_maker ------------------------> flux.strategies.registry

equities_taker ------------------------> flux.common
equities_taker ------------------------> flux.strategies.shared
equities_taker ------------------------> flux.strategies.registry

flux.strategies.shared ----------------> flux.common
flux.strategies.registry --------------> constant metadata + lazy class loading only

flux.runners.shared -------------------> flux.common
flux.runners.shared -------------------> flux.strategies.registry
flux.runners.shared -------------------> strategy outputs / external contracts
```

Strategies do not depend on `flux.runners.shared` in the target architecture.

## Layer Ownership

### `flux.common`

Owns repo-wide generic primitives that are not strategy-family-specific:

- quantity-unit normalization and exposure conversion primitives
- repo-wide runtime-param registry composition helpers
- strategy capability models and normalized strategy-contract identity helpers
- first-class market or instrument identity contracts consumed by later execution helpers
- account-projection read-model helpers
- market identity or capability normalization introduced by this wave
- generic value objects and low-level helpers that should not live under one strategy family

Must not own:

- family-specific defaults
- strategy-family payload assembly
- strategy-family execution policy

### `flux.strategies.shared`

Owns reusable strategy-runtime primitives:

- shared config adapters or mixins
- shared observability helpers
- shared topic constants and payload-schema helpers for already-frozen legacy external contracts under an explicit `observability/contracts` namespace
- shared managed-order primitives
- strategy-facing inventory adapters that compose `flux.common` helpers
- shared venue-policy parsing that consumes normalized inputs

Must not own:

- raw quantity/projection truth already owned by `flux.common`
- family-specific state machines
- family-specific quoting or hedging policy

### `flux.strategies.registry`

Owns strategy identity metadata and lookup:

- `FluxStrategyIdentity`
- `FluxStrategySpec`
- strategy-id and `param_set` mapping
- lazy strategy-class loading so import-time callers do not pull optional dependencies accidentally

`FluxStrategySpec` must stay data-only. It may hold:

- strategy id
- `param_set`
- config type path
- class import path
- capability metadata
- deploy-surface metadata needed for pilot/bootstrap workflows

It must not hold imported strategy class objects in the registry metadata table.

Must not own:

- family business logic
- runner-owned orchestration

### `flux.runners.shared`

Owns runner services only:

- profile-account runners
- runner-owned IBKR reference-balance provider and cache
- quote-feed supervisors
- runner bootstrap helpers

Runners may consume strategy registry metadata and external strategy contracts, but strategies must not depend on runner packages.

## External Contract Freeze

These contracts are frozen across the wave unless a child PR documents an explicit bug fix or additive-only change.

| Surface | Current contract | Wave rule |
| --- | --- | --- |
| Topic names | `flux.makerv3.state`, `event`, `alert`, `market_bbo`, `fv`, `balances`, `order_intent`, `trade` | preserve names; helper ownership may move |
| API strategy payloads | `/api/v1/signals`, profile payloads, realtime payloads | preserve semantics; additive fields only unless bug fix |
| Balances API | `/api/v1/balances` strategy and profile views | preserve shape and source-of-truth meaning |
| Runtime param identity | `param_set` values such as `makerv3`, `makerv4`, `equities_maker`, `equities_taker` | preserve values |
| Blocked-state semantics | `quote_blockers`, `quote_progress`, `quote_health`, state names | preserve names and meaning unless fixing a documented bug |
| Telemetry | `quote_cycle`, `order_action`, `execution_fill` semantics and linkage fields | preserve semantics; additive enrichment only |
| Bridge suffix routing | bridge mapping from full topics to suffix topics | preserve routing behavior |

Important nuance: topic constants for frozen legacy contracts may move into shared observability ownership, but family-specific payload assembly and publication timing stay family-local unless explicitly extracted.

## Non-Negotiable Execution Rules

1. No shims and no dual ownership.
2. Shared modules may not import from strategy families.
3. Every moved primitive migrates all live consumers in the same PR.
4. Behavior changes require explicit bug-fix rationale.
5. Contract-first tests land before each extraction.
6. Shared/common public modules need docstrings and direct invariant tests.
7. Each runtime-code-touching PR must define:
   - release-blocking environments
   - affected pilot surfaces
   - pilot smoke bundle
   - PR-local rollback note
8. The wave must add and maintain an executable architecture-boundary test with declarative forbidden-import and deleted-path manifests for extracted primitives.

## Release-Blocking Environments

| Environment | Purpose | Minimum health check |
| --- | --- | --- |
| `default-unit` | normal isolated unit/test lane for non-IB surfaces | `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest ...` |
| `ibkr-unit` | required for PRs that touch Makerv4 or equities imports/tests | `python -c "import ibapi"` must succeed before the PR may merge |
| `pilot` | pinned release validation for affected live stacks | deploy PR head to pilot only, validate PR-specific smoke bundle, then promote |

`PR1` through `PR4` are not mergeable without a valid `ibkr-unit` lane.

## Multi-Market Design Rule

This wave does not claim “multi-market ready” unless the extracted shared/common surfaces avoid new string-only ownership traps.

Practical implications:

1. `PR0` fixes the current borrow-cap safety path with a narrow shared parser and freezes the contract.
2. `PR1` owns the import-boundary cleanup, the `FluxStrategyCapabilities` move into `flux.common`, the data-only `FluxStrategySpec`, and the first-class `flux.common.market_identity` contract later PRs consume.
3. `PR4` is where migrated Makerv3 and Makerv4 execution helpers must stop using duplicated `-SPOT.` / `-PERP.` / `_SPOT` heuristics and consume the common typed market-identity inputs instead.
4. No new shared/common API may assume PLUME or one venue/instrument family is the default case.

## PR Stack

### PR0. Baseline safety green and contract freeze

Purpose:

- fix the current MakerV3 borrow-cap mismatch
- freeze the affected blocked-state, alert, API, and readiness contracts
- add the narrow shared venue-policy parser needed for that fix

### PR1. Shared strategy foundations

Purpose:

- move cross-family types and config bases out of MakerV3 ownership
- keep runtime-param composition repo-wide under `flux.common`
- move `FluxStrategyCapabilities`, first-class market identity, and normalized strategy-contract helpers into `flux.common`
- fix `flux.strategies.__init__` and `flux.strategies.registry` so strategy metadata loads lazily and does not leak optional IBKR imports
- keep `FluxStrategySpec` data-only rather than import-time class-driven

### PR2. Shared-account projection ownership realignment

Purpose:

- move shared-account projection helpers out of `strategies/shared`
- make account-projection lookup clearly common/read-model owned
- move the runner-owned IBKR reference-balance provider out of `strategies.shared.equities_arb` and into `flux.runners.shared`

### PR3. Shared observability platform

Purpose:

- move reusable serialization, alerting, publish helpers, and frozen legacy topic/schema contracts into explicit shared observability-contract ownership
- remove `shared -> makerv3` and `makerv4 -> makerv3 publisher/constants` dependency edges

### PR4. Shared execution primitives

Purpose:

- move managed-order primitives into shared ownership
- move strategy-facing inventory adapters out of MakerV3 ownership while keeping raw unit normalization and projection truth in `flux.common`
- replace duplicated Makerv3/Makerv4 market-type heuristics in the migrated execution path with the typed common market-identity contract

### PR5. MakerV3 quote pipeline split

Purpose:

- split `quote_engine.refresh_quotes` into coherent family-local collaborators after shared ownership cleanup is complete

### PR6. MakerV3 strategy decomposition

Purpose:

- shrink the remaining MakerV3 strategy god object after the quote hot path is split

### PR7. Residual docs, docstrings, and cleanup

Purpose:

- finish residual documentation and docstrings after the architecture is stable
- close remaining late-wave invariant gaps without backfilling obligations that earlier PRs should have carried

## PR Ordering Rationale

1. `PR0` is mandatory because the current baseline is not releasable.
2. `PR1` must land early because lazy registry/import boundaries affect every later verification story.
3. `PR2` comes before execution extraction so account-projection truth is already in the correct layer.
4. `PR3` and `PR4` finish the reusable shared surfaces before any MakerV3-local hot-path decomposition.
5. `PR5` and `PR6` then operate against the final shared ownership graph.
6. `PR7` is residual documentation closeout, not a substitute for earlier PR proof.

## Wave-Level Verification Matrix

Every PR must update its relevant rows.

| Surface | Environment | Required invariant |
| --- | --- | --- |
| Architecture boundaries | `default-unit` | forbidden import directions and deleted-path manifests fail fast in tests |
| MakerV3 core suite | `default-unit` | baseline green and borrow-cap bug fixed |
| Shared strategy helpers | `default-unit` | extracted shared modules have direct tests |
| Runtime param/common contracts | `default-unit` | shared type/config/param composition remains stable |
| Registry/import boundary | `default-unit` and `ibkr-unit` | non-IB callers do not pull optional IBKR imports accidentally; IBKR-enabled callers still resolve the same strategy specs |
| Makerv4 strategy coverage | `ibkr-unit` | behavior unchanged except approved bug fixes |
| Equities family coverage | `ibkr-unit` | equities families consume shared platform directly |
| API payload contracts | `default-unit` | state, balances, quote blockers, and realtime contracts remain honest |
| Golden contract fixtures | `default-unit` and `ibkr-unit` where relevant | representative payloads and schemas remain byte-for-byte or field-for-field stable |
| Runner/readiness contracts | `default-unit` and `pilot` where relevant | topic wiring and blocked-state/readiness semantics stay intact |
| Pilot validation | `pilot` | each PR is deployable, smokes cleanly, and is revert-safe on its own |
| Docs/runbooks | `default-unit` | permanent docs match real contracts and ownership |

## Rollout And Rollback Model

### Rollout

1. Build one pinned pilot release from the PR head.
2. Deploy the exact process-level units named in the child plan from that same release.
3. Follow the child plan promotion order so unsupported mixed-version states are never entered.
4. Run the PR-specific pilot smoke bundle.
5. Promote only after pilot is green.

### Rollback

1. Revert the whole PR.
2. Redeploy the previous pinned release.
3. Do not leave shared/common ownership moved while reverting only one consumer.

### Unsupported states

1. Mixed-revision producer/consumer rollouts for the same extracted contract
2. partial rollback that keeps shared helpers but restores only one family
3. deploys from mutable worktrees instead of pinned releases

## Open Assumptions

1. `PR1` through `PR4` require access to an `ibkr-unit` environment with `ibapi` installed.
2. Existing shared quote-health and runner quote-feed supervisor behavior remains input to this wave rather than redesign scope.
3. The wave preserves legacy topic namespaces even if helper ownership moves.

## Design Confidence Summary

High confidence:

- the dependency inversion is concrete and verified on current `main`
- `PR0` is mandatory
- registry/import-boundary cleanup belongs in `PR1`
- shared-account projection ownership belongs in `flux.common`, not `strategies/shared`
- per-PR pilot and rollback sections are required for a credible no-shim plan
- architecture-boundary enforcement and golden fixtures should be planned up front rather than added ad hoc during implementation

Medium confidence:

- exact helper filenames inside `PR3` and `PR4` may shift slightly during implementation
- some final Makerv3-local helper splits between `PR5` and `PR6` may move by a file or two without changing wave shape

Low-confidence items intentionally deferred:

- full cross-family market-structure unification beyond what this wave needs

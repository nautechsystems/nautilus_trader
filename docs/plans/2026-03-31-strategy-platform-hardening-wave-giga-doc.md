# Strategy Platform Hardening Wave Giga Doc

> Canonical full packet for external review. Regenerated from the current post-`main` planning set on 2026-03-31.

## Included Sources

- `docs/prd/2026-03-31-strategy-platform-hardening-wave.md`
- `docs/plans/2026-03-31-strategy-platform-hardening-wave-design.md`
- `docs/plans/2026-03-31-strategy-platform-hardening-wave.md`
- `docs/plans/2026-03-31-strategy-platform-hardening-wave-review-packet.md`
- `docs/plans/2026-03-31-strategy-platform-hardening-wave-pr0-baseline-safety-and-contract-freeze.md`
- `docs/plans/2026-03-31-strategy-platform-hardening-wave-pr1-shared-strategy-foundations.md`
- `docs/plans/2026-03-31-strategy-platform-hardening-wave-pr2-shared-account-projection-ownership.md`
- `docs/plans/2026-03-31-strategy-platform-hardening-wave-pr3-shared-observability-platform.md`
- `docs/plans/2026-03-31-strategy-platform-hardening-wave-pr4-shared-execution-primitives.md`
- `docs/plans/2026-03-31-strategy-platform-hardening-wave-pr5-makerv3-quote-pipeline-split.md`
- `docs/plans/2026-03-31-strategy-platform-hardening-wave-pr6-makerv3-strategy-decomposition.md`
- `docs/plans/2026-03-31-strategy-platform-hardening-wave-pr7-docstrings-runbooks-and-cleanup.md`


---

## Source: `docs/prd/2026-03-31-strategy-platform-hardening-wave.md`

# Strategy Platform Hardening Wave PRD

**Date:** 2026-03-31

## Summary

This wave hardens the current Flux strategy families for production while turning `systems/flux/flux/strategies/shared/` into a real strategy platform instead of a thin helper bucket.

The primary debt source is still MakerV3, but the current repo state is a cross-family problem:

- `makerv4` imports reusable inventory, managed-order, publisher, constant, and config surfaces from `makerv3`
- `equities_maker` and `equities_taker` still depend on MakerV3-owned types and runtime-param templates
- `shared` modules still import MakerV3 publisher helpers
- package/registry imports eagerly load IBKR-dependent strategy code into non-IB test paths
- operator and telemetry contracts are spread across code, tests, API payload logic, bridge/runners, and runbooks rather than frozen in one reviewed place

This wave must leave the repo in a state where future shared strategy improvements land once in `shared` and are consumed by strategy families without another MakerV3-first extraction cycle.

For this document, `PLUME-shaped` means a shared or common abstraction still assumes one concrete symbol family, venue pair, or instrument-id suffix pattern is the canonical case instead of taking neutral metadata or capability inputs.

## Why Now

The current architecture is too coupled to scale safely:

1. New symbol or market work still risks touching MakerV3-owned surfaces that Makerv4 and equities already consume.
2. The hottest MakerV3 path is still too monolithic to refactor safely without stronger tests and module boundaries.
3. The current verification story is weak:
   - MakerV3 has an existing red safety-path contract
   - Makerv4/equities collection currently drags in IBKR dependencies too early in the default unit environment
4. The repo already has post-`#91` equities shared-node and market-data recovery work on `main`, so this is the right point to clean the platform ownership graph before more family work accumulates.

## Problem Statement

Today the strategy layer has four structural problems.

### 1. Reusable logic is owned by the wrong family

Reusable primitives still live under `makerv3`:

- runtime/config contracts
- inventory/exposure helpers
- managed-order utilities
- alert/json publishing helpers
- topic/constants surfaces

That forces downstream strategy families to depend on a family-local implementation instead of a stable shared platform.

### 2. Dependency direction is inverted

`shared` is not dependency-bottom today. It still imports from MakerV3 in places, which means the supposed platform layer is coupled upward into one strategy family.

### 3. External/operator contracts are real but under-governed

The following surfaces already matter operationally and must be treated as frozen contracts unless an explicit bug fix is approved:

- `flux.makerv3.*` topics
- `quote_cycle`, `order_action`, and `execution_fill` telemetry semantics
- blocked-state and quote-health payload fields consumed by API/readiness/UI logic
- balances snapshots consumed by API and portfolio views
- runtime `param_set` identifiers

### 4. Multi-market generalization is incomplete

The repo is still too heuristic-driven for the stated goal of supporting more symbols and markets cleanly:

- product type is still inferred from instrument-id suffixes in strategy code
- venue-specific block handling is still driven by raw reason parsing in strategy-local modules
- shared APIs are not yet explicitly capability-driven

## Goals

### Primary goals

1. Make the current MakerV3, MakerV4, `equities_maker`, and `equities_taker` code easier to review, test, and change without altering intended behavior.
2. Eliminate reusable cross-family imports from MakerV3-owned modules by moving the real shared primitives into `systems/flux/flux/strategies/shared/`.
3. Preserve production behavior and operator-facing contracts except for explicit bug fixes.
4. Leave every PR in the wave independently mergeable, deployable, and revertable with no compatibility shims.

### Platform goals

1. Make `shared` the stable home for cross-family strategy primitives.
2. Ensure future shared strategy work can land once and fan out to strategy families without another large ownership migration.
3. Use `flux.common`, `flux.strategies.shared`, and lazy `flux.strategies.registry` ownership coherently so shared improvements do not route back through MakerV3.
4. Make market and instrument identity first-class in `flux.common` before execution extraction so later PRs migrate onto one typed normalization surface instead of prose-only guidance.
5. Reduce symbol-, venue-, and market-specific hardcoding so adding more markets does not require string-heuristic edits in strategy cores.

## Non-Goals

This wave is not the place to:

1. Change external topic namespaces away from `flux.makerv3.*`.
2. Redesign Fluxboard or introduce new API resources.
3. Build a standalone strategy platform service or distributed market-data platform.
4. Add new venues, new strategy families, or new quoting features beyond explicit bug fixes and structural hardening.
5. Rewrite Makerv4/equities trading behavior beyond what is required to consume the new shared platform directly.

## Constraints

The user’s execution constraints are mandatory:

1. No compatibility shims.
2. Straight-to-prod PRs.
3. Each PR must be independently mergeable and releasable.
4. No behavior changes unless fixing a clear bug or contract mismatch.
5. Plan and execute from post-`#91` `main`.
6. Scope includes migration of current Makerv3, Makerv4, `equities_maker`, `equities_taker`, and `shared/*` consumers.

## Target Users

### Direct users

- strategy engineers changing MakerV3, MakerV4, or equities families
- operators relying on state/alert/balances/readiness truth
- reviewers trying to reason about refactors without re-deriving the whole dependency graph

### Secondary users

- future engineers building new shared strategy features
- external reviewers evaluating architecture and rollout safety

## Success Criteria

The wave is successful only if all of the following are true.

### Architecture and ownership

1. No module under `systems/flux/flux/strategies/shared/` imports from `makerv3`, `makerv4`, `equities_maker`, or `equities_taker`.
2. No cross-family reusable primitive is still owned only by MakerV3.
3. MakerV4 and equities families no longer import MakerV3-owned reusable modules, types, or config bases.
4. Strategy identity/registry surfaces no longer require eager import of heavy IBKR strategy code for non-IB use cases.
5. `FluxStrategySpec` is data-only metadata rather than an import-time class object container.
6. An executable architecture-boundary test enforces forbidden import directions and deleted-path manifests for extracted primitives.

### Production hardening

1. The existing MakerV3 borrow-cap alert/state contract mismatch is resolved and locked in tests.
2. The wave defines and follows a concrete verification matrix across strategy families, API payloads, bridge/runners, and docs contracts.
3. Each PR has a documented revert-safe rollout model with no unsupported partial-deploy assumptions.
4. Representative frozen contracts are preserved with golden fixtures, not only point assertions.

### Operator contract governance

1. Topic names, telemetry semantics, state payload fields, and balances/API surfaces touched by the wave are inventoried in a contract-freeze table.
2. Any intentional contract change is explicitly called out as a bug fix or additive field only.
3. Representative frozen payloads and schemas have golden fixtures for blocked-state, balances, observability envelopes, and parameter-schema surfaces.

### Multi-market generalization

1. Product-type and market-identity decisions needed by the wave flow through one typed `flux.common` contract instead of duplicated raw string heuristics in strategy families.
2. Shared primitives introduced by the wave are symbol- and venue-agnostic unless explicitly venue-scoped.
3. Tests prove the shared abstractions are not PLUME-shaped by construction.

## Dependencies

### Hard dependencies

1. Current post-`#91` `main`
2. Existing shared quote-health and node quote-feed supervisor work already on `main`
3. Existing API/readiness/bridge contract tests and runbooks

### Execution dependency

The wave depends on one planning assumption:

- where a test slice genuinely requires IBKR adapter dependencies, the implementation environment must provide an `ibkr-unit` lane with `ibapi`; the wave still must remove accidental IBKR dependency leaks from non-IB lanes

## Risks

### Highest risks

1. A large extraction can accidentally change operator payloads even if trading logic is unchanged.
2. Shared-module extractions can create circular imports if the new ownership boundaries are not enforced strictly.
3. Multi-family migrations can look behavior-preserving while quietly shifting default runtime-param or config semantics.
4. A no-shim approach raises rollback sensitivity if a PR changes external contracts or deploy ordering assumptions.

### Mitigations

1. Freeze the operator surfaces in the design docs before implementation starts.
2. Use contract-first tests before each extraction.
3. Require a cross-family verification slice in every PR, not only the module’s home family.
4. Add an executable architecture-boundary gate so dependency-direction regressions fail in CI rather than relying on reviewer memory.
5. Treat revert safety and promotion order as design requirements, not post-merge notes.

## Release Bar

Do not call the wave complete unless:

1. the master verification matrix is green in the required environments
2. every child PR plan is executed or explicitly deferred out of the wave with written rationale
3. the final dependency graph matches the target architecture in the design doc
4. permanent docs reflect the real platform ownership and operator contracts

## Out-of-Wave Follow-Ups

Items intentionally left for later if the current wave succeeds:

1. deeper shared market-structure extraction beyond what is needed to remove current hardcoded heuristics
2. new strategy-family builds on top of the shared platform
3. broader CI lane normalization for all optional venue dependencies



---

## Source: `docs/plans/2026-03-31-strategy-platform-hardening-wave-design.md`

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



---

## Source: `docs/plans/2026-03-31-strategy-platform-hardening-wave.md`

# Strategy Platform Hardening Wave Implementation Overview

Canonical review set note: only documents named `2026-03-31-strategy-platform-hardening-wave*.md` are part of the active review packet for this wave. Older March 31 `wave-*` drafts are superseded scratch material and are not part of the canonical stack.

**Goal:** Execute the strategy-platform hardening wave as a sequence of independently releasable no-shim PRs that clean up current Makerv3/Makerv4/equities debt while establishing durable shared strategy layers.

**Architecture:** Start by making the current baseline releasable and freezing operator contracts. Then remove Makerv3-owned shared foundations in layers: contract types and config plus lazy registry/import boundaries, shared-account read-model ownership, observability, and execution primitives. Only after dependency direction is clean do the Makerv3-local hot-path and god-object decompositions land.

**Context Docs:**
- PRD: `docs/prd/2026-03-31-strategy-platform-hardening-wave.md`
- Design: `docs/plans/2026-03-31-strategy-platform-hardening-wave-design.md`
- Review Packet: `docs/plans/2026-03-31-strategy-platform-hardening-wave-review-packet.md`

## Wave Order

1. `PR0` baseline safety green and venue-policy contract freeze
2. `PR1` shared strategy foundations
3. `PR2` shared-account projection ownership realignment
4. `PR3` shared observability platform
5. `PR4` shared execution primitives
6. `PR5` Makerv3 quote pipeline split
7. `PR6` Makerv3 strategy decomposition
8. `PR7` residual docstrings, runbooks, and cleanup

## Current Verified Baseline

Verified on `main` on 2026-03-31:

| Surface | Command | Result |
| --- | --- | --- |
| Makerv3 strategy suite | `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/strategies/makerv3 -q` | `1 failed, 289 passed` |
| Makerv4/equities/shared strategy bundle | `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/strategies/makerv4 tests/unit_tests/flux/strategies/equities_maker tests/unit_tests/flux/strategies/equities_taker tests/unit_tests/flux/strategies/shared -q` | collection blocked in this environment because `ibapi` is missing |

This is why the wave begins with `PR0`.

## Wave-Level Gates

These gates apply to every PR:

1. The PR migrates every current consumer of the extracted primitive in the same diff.
2. The old Makerv3-owned path for that primitive is removed immediately.
3. The PR carries its own contract tests, operator note, verification commands, and rollback note.
4. The PR is pilot-deployable and revert-safe on its own.
5. Shared modules remain dependency-bottom and import no family-local strategy packages.
6. The wave maintains an executable architecture-boundary test so forbidden import directions and deleted paths fail fast.

## Release-Blocking Environments

| Environment | Required for | Rule |
| --- | --- | --- |
| `default-unit` | every PR | focused isolated unit slices with `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1` |
| `ibkr-unit` | `PR1` through `PR4` | `python -c "import ibapi"` must succeed before merge; missing `ibapi` is not a waiver |
| `pilot` | every runtime-code PR | deploy one pinned pilot release from the PR head and run the PR-specific smoke bundle; `PR7` is docs/docstrings/tests only and is exempt unless it stops being non-runtime |

## Wave-Level Verification Matrix

| PR | Surfaces | Minimum verification | Expected invariant |
| --- | --- | --- | --- |
| PR0 | Makerv3 safety path, venue-policy helper, API blocker semantics, readiness | full Makerv3 suite plus targeted API payload and readiness slices in `default-unit`, plus pilot smoke for tokenmm/state consumers | baseline green, borrow-cap contract fixed, blocker semantics frozen |
| PR1 | Makerv3, Makerv4, equities maker/taker, API params/profile routing, registry/import boundary, common capability/identity helpers | architecture-boundary tests, runtime-param, config, registry, common identity/capability, golden param-schema fixtures, and API params/profile tests across `default-unit` and `ibkr-unit`, plus pilot smoke for tokenmm/equities/API consumers | no Makerv3-owned type/config/param-builder imports remain in migrated consumers; eager IBKR import leak removed; registry no longer depends on `strategies.shared.capabilities`; `FluxStrategySpec` stays data-only |
| PR2 | common/read-model helpers, runner-owned reference balances, Makerv3, Makerv4, profile-account consumers | common account-projection tests plus runner/API/strategy inventory/reference-balance slices across `default-unit` and `ibkr-unit`, plus pilot smoke for tokenmm/equities/API consumers | shared-account read-model ownership no longer lives under `strategies/shared`; runner-owned reference balances no longer live under `strategies.shared.equities_arb` |
| PR3 | shared observability, Makerv3, Makerv4, equities/shared family helpers, API realtime/trades, readiness/bridge consumers | architecture-boundary tests, shared helper tests, golden payload fixtures, strategy observability tests, runner bridge/readiness tests, and API payload/realtime slices across `default-unit` and `ibkr-unit`, plus pilot smoke for tokenmm/equities/API consumers | topic names and payload keys preserved while helper ownership moves into explicit observability-contract ownership |
| PR4 | shared managed orders, shared inventory adapters, Makerv3, Makerv4, API inventory projections, common market identity normalization | architecture-boundary tests, direct shared tests, golden inventory/order-intent fixtures, common market-identity, strategy, and API inventory/order-safety slices across `default-unit` and `ibkr-unit`, plus pilot smoke for tokenmm/equities/API consumers | reusable execution primitives no longer live in Makerv3 and migrated helpers no longer rely on duplicated suffix heuristics |
| PR5 | Makerv3 quote pipeline | Makerv3 quote-engine, order-intent, and observability slices in `default-unit`, plus pilot smoke for tokenmm quoting/telemetry | hot-path split is behavior-preserving |
| PR6 | Makerv3 lifecycle/order state/state export | Makerv3 lifecycle, reconciliation, runtime-params, and observability slices in `default-unit`, plus pilot smoke for tokenmm lifecycle/state export | strategy class shrinks without contract drift |
| PR7 | docs, runbooks, residual invariants | docs tests, residual unit invariants, docstring/lint checks in `default-unit` | final docs now match the architecture; no earlier PR obligations deferred here |

## Operator Contract Freeze

These surfaces are frozen across the wave unless a PR explicitly calls out an additive-only change or a bug fix:

- `flux.makerv3.state`
- `flux.makerv3.event`
- `flux.makerv3.alert`
- `flux.makerv3.market_bbo`
- `flux.makerv3.fv`
- `flux.makerv3.balances`
- `flux.makerv3.order_intent`
- `flux.makerv3.trade`
- `quote_progress`
- `quote_blockers`
- `quote_health`
- trade quantity fields: `qty`, `qty_venue`, `qty_base`, `qty_conversion_status`, `qty_conversion_source`
- `/api/v1/signals`
- `/api/v1/balances`
- `/api/v1/param-schema`
- `/api/v1/strategies/<id>/parameters`
- `/equities` profile contract

## Rollout And Rollback Model

### Rollout

- build one pinned pilot release from the PR head
- move the exact child-plan deploy units together from that same release, typically `flux.runners.tokenmm.run_node`, `flux.runners.equities.run_node`, and `flux.api.app` when those processes are in scope
- follow the child-plan promotion order so unsupported mixed-version states are never entered
- validate the PR-specific verification bundle
- promote only after pilot is green

### Rollback

- revert the whole PR
- redeploy the previous pinned release
- do not run mixed-version partial rollback across affected family consumers

This works only because the wave freezes public contract names by default.

## Cross-PR Risks

| Risk | Mitigation |
| --- | --- |
| hardening the wrong boundary for params or shared-account projection | fixed explicitly in the design and early PR order |
| claiming “independently releasable” without concrete proof | wave-level matrix plus child-plan verification tables and explicit deploy-unit order |
| operator regressions from shared extraction | contract freeze table plus API and strategy export tests in every affected PR |
| false multi-market confidence | explicit anti-hardcoding acceptance criteria, typed market identity, and non-PLUME tests |

## Child Plans

- `docs/plans/2026-03-31-strategy-platform-hardening-wave-pr0-baseline-safety-and-contract-freeze.md`
- `docs/plans/2026-03-31-strategy-platform-hardening-wave-pr1-shared-strategy-foundations.md`
- `docs/plans/2026-03-31-strategy-platform-hardening-wave-pr2-shared-account-projection-ownership.md`
- `docs/plans/2026-03-31-strategy-platform-hardening-wave-pr3-shared-observability-platform.md`
- `docs/plans/2026-03-31-strategy-platform-hardening-wave-pr4-shared-execution-primitives.md`
- `docs/plans/2026-03-31-strategy-platform-hardening-wave-pr5-makerv3-quote-pipeline-split.md`
- `docs/plans/2026-03-31-strategy-platform-hardening-wave-pr6-makerv3-strategy-decomposition.md`
- `docs/plans/2026-03-31-strategy-platform-hardening-wave-pr7-docstrings-runbooks-and-cleanup.md`

## Confidence Summary

### High confidence

- the dependency direction needs this class of cleanup
- `PR0` is mandatory
- param contracts must stay repo-wide
- shared-account read-model ownership must move out of strategy-shared code
- per-PR verification and rollback notes must move with each PR

### Medium confidence

- exact helper filenames inside the observability and inventory-math extractions may shift slightly during implementation
- the exact final split of some Makerv3-local helper methods between PR5 and PR6 may need a small implementation-time adjustment



---

## Source: `docs/plans/2026-03-31-strategy-platform-hardening-wave-review-packet.md`

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



---

## Source: `docs/plans/2026-03-31-strategy-platform-hardening-wave-pr0-baseline-safety-and-contract-freeze.md`

# Strategy Platform Hardening Wave PR0 Baseline Safety And Contract Freeze Implementation Plan

> **For the execution agent:** REQUIRED SUB-SKILL: Before implementing this plan, choose exactly one execution mode and use the matching skill: `superpowers:subagent-driven-development` for same-session execution or `superpowers:executing-plans` for a separate-session handoff.
> **Progress:** The Progress Tracker in this document is the execution source of truth and must be updated on every state change.

**Goal:** Make the baseline releasable by fixing the current Makerv3 borrow-cap safety-path mismatch, freezing the venue-policy and quote-blocker contract, and proving the safety path is multi-market rather than PLUME-shaped.

**Architecture:** Add one explicit shared venue-policy helper for borrow-cap and exchange-code parsing, migrate Makerv3 onto it, and lock the alert/state/API contract in tests before any broader extraction work starts. This PR is allowed to change behavior only for the currently failing borrow-cap path, and only to align the implementation with the frozen contract below.

**Tech Stack:** Python strategy/runtime code, shared strategy helpers, Flux API payload builders, pytest.

**Context Docs:**
- Design: `docs/plans/2026-03-31-strategy-platform-hardening-wave-design.md`
- PRD: `docs/prd/2026-03-31-strategy-platform-hardening-wave.md`
- Relevant specs/runbooks: `systems/flux/docs/makerv3.md`, `docs/plans/2026-03-31-strategy-platform-hardening-wave.md`

**Decision Summary:**
- `PR0` is mandatory because the current Makerv3 suite is not releasable.
- Venue-policy parsing moves into an explicit shared helper now rather than after other extractions.
- The public operator and API contract for quote blockers must be pinned in the same PR that fixes the failing path.
- The shared venue-policy helper must return a structured result like `reason_code`, `affected_side`, and `venue_code`; raw rejection text may be preserved for logs and alerts but not re-parsed downstream.

## Borrow-Cap Contract Frozen In This PR

This PR must implement and freeze this exact contract:

- a spot borrow-cap rejection blocks only the affected ask or `SELL` side
- only affected ask managed orders are cancelled
- overall strategy state remains `running`
- `bot_on` remains `true`
- state and API payloads record a side-local `spot_borrow_cap` blocker
- one actionable alert plus structured event is emitted under cooldown gating

This PR does not get to defer that choice to implementation time.

## Release Gates

| Environment | Required for merge? | Requirement |
| --- | --- | --- |
| `default-unit` | yes | Makerv3, API, and readiness proof must be green |
| `pilot` | yes | deploy one pinned tokenmm pilot release from the PR head and validate the smoke bundle below |

## Affected Pilot Surfaces

- tokenmm Makerv3 strategy stack
- `/api/v1/signals` blocked-state consumers
- readiness and bridge consumers of `quote_blockers` truth

## Pilot Deploy Units And Promotion Order

Deploy units:

- `flux.runners.tokenmm.run_node`
- `flux.api.app` if pilot signals or readiness surfaces are served separately

Promotion order:

1. build one pinned pilot release from the exact PR head
2. deploy `flux.runners.tokenmm.run_node` and `flux.api.app` from that same release when both are in scope
3. validate blocker-state, readiness, and alert-cooldown behavior
4. promote the same units together only after the smoke bundle is clean

## Pilot Smoke Bundle

1. Deploy the tokenmm pilot stack from the exact PR head.
2. Confirm the strategy starts and remains `running`.
3. Confirm a borrow-cap rejection or controlled replay fixture produces an ask-side-only `spot_borrow_cap` blocker.
4. Confirm `/api/v1/signals` and readiness surfaces remain coherent with the same blocker state.
5. Confirm the alert path emits at most one actionable alert per cooldown window.

## PR-Local Rollback

- revert the whole PR
- redeploy the previous pinned pilot release
- do not mix reverted Makerv3 logic with new blocker-contract consumers

## Progress Tracker

**Source of truth:** Update this table whenever task state changes. Do not rely on memory, chat history, or TodoWrite alone.

| Task | Status | Owner | Depends On | Write Scope | Lane Branch | Worktree Path | Commit / Diff | Verification | Notes / Last Update |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Overall | not_started | unassigned | none | `systems/flux/flux/strategies/makerv3`, `systems/flux/flux/strategies/shared`, `systems/flux/flux/api`, `tests/unit_tests/flux/strategies/makerv3`, `tests/unit_tests/flux/strategies/shared`, `tests/unit_tests/flux/api`, `systems/flux/docs` | `wave/pr0-baseline-safety` | `.worktrees/strategy-platform-pr0` | none | not_run | Plan created |
| Task 1: Lock the failing borrow-cap contract in red tests | not_started | unassigned | none | `tests/unit_tests/flux/strategies/makerv3/test_order_safety.py`, `tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py`, `tests/unit_tests/flux/api/test_payloads.py`, `tests/unit_tests/flux/api/test_payload_snapshots.py`, `systems/flux/docs/makerv3.md` | `wave/pr0-baseline-safety` | `.worktrees/strategy-platform-pr0` | none | not_run | Plan created |
| Task 2: Extract shared venue-policy parsing and migrate Makerv3 | not_started | unassigned | Task 1: Lock the failing borrow-cap contract in red tests | `systems/flux/flux/strategies/shared/venue_policy.py`, `systems/flux/flux/strategies/makerv3/failures.py`, `systems/flux/flux/strategies/makerv3/strategy.py`, `tests/unit_tests/flux/strategies/shared/test_venue_policy.py` | `wave/pr0-baseline-safety` | `.worktrees/strategy-platform-pr0` | none | not_run | Plan created |
| Task 3: Align alerts, state exports, and API blocker semantics | not_started | unassigned | Task 2: Extract shared venue-policy parsing and migrate Makerv3 | `systems/flux/flux/strategies/makerv3/strategy.py`, `systems/flux/flux/strategies/makerv3/publisher.py`, `systems/flux/flux/api/_payloads_signals.py`, `tests/unit_tests/flux/api/test_payloads.py`, `systems/flux/docs/makerv3.md` | `wave/pr0-baseline-safety` | `.worktrees/strategy-platform-pr0` | none | not_run | Plan created |
| Task 4: Run baseline verification and write rollback note | not_started | unassigned | Task 3: Align alerts, state exports, and API blocker semantics | `docs/plans/2026-03-31-strategy-platform-hardening-wave-pr0-baseline-safety-and-contract-freeze.md`, `systems/flux/docs/makerv3.md` | `wave/pr0-baseline-safety` | `.worktrees/strategy-platform-pr0` | none | not_run | Plan created |

---

### Task 1: Lock the failing borrow-cap contract in red tests

**Files:**
- Modify: `tests/unit_tests/flux/strategies/makerv3/test_order_safety.py`
- Modify: `tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py`
- Modify: `tests/unit_tests/flux/api/test_payloads.py`
- Create: `tests/unit_tests/flux/api/test_payload_snapshots.py`
- Modify: `systems/flux/docs/makerv3.md`

**Dependencies:** `none`

**Write Scope:** `tests/unit_tests/flux/strategies/makerv3/test_order_safety.py`, `tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py`, `tests/unit_tests/flux/api/test_payloads.py`, `tests/unit_tests/flux/api/test_payload_snapshots.py`, `systems/flux/docs/makerv3.md`

**Verification Commands:**
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/strategies/makerv3/test_order_safety.py tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py -q`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/api/test_payloads.py -q -k 'quote_blockers or borrow'`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/api/test_payload_snapshots.py -q -k 'borrow_cap or quote_blockers'`

**Step 1: Write the failing tests**

Lock the intended contract for:

- which side is blocked on spot borrow cap
- whether an actionable alert is emitted
- which blocker fields appear in state and API payloads
- one representative golden payload fixture for blocked-state export
- multi-market behavior using non-PLUME fixture symbols

**Step 2: Run tests to verify they fail**

Run the commands above and confirm the current borrow-cap path is red.

**Step 3: Record the contract in docs**

Update `systems/flux/docs/makerv3.md` to state the frozen borrow-cap operator contract before implementation changes land.

**Step 4: Commit**

```bash
git add tests/unit_tests/flux/strategies/makerv3/test_order_safety.py \
  tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py \
  tests/unit_tests/flux/api/test_payloads.py \
  tests/unit_tests/flux/api/test_payload_snapshots.py \
  systems/flux/docs/makerv3.md
git commit -m "test: lock borrow cap safety contract"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.
### Task 2: Extract shared venue-policy parsing and migrate Makerv3

**Files:**
- Create: `systems/flux/flux/strategies/shared/venue_policy.py`
- Modify: `systems/flux/flux/strategies/makerv3/failures.py`
- Modify: `systems/flux/flux/strategies/makerv3/strategy.py`
- Create: `tests/unit_tests/flux/strategies/shared/test_venue_policy.py`

**Dependencies:** `Task 1: Lock the failing borrow-cap contract in red tests`

**Write Scope:** `systems/flux/flux/strategies/shared/venue_policy.py`, `systems/flux/flux/strategies/makerv3/failures.py`, `systems/flux/flux/strategies/makerv3/strategy.py`, `tests/unit_tests/flux/strategies/shared/test_venue_policy.py`

**Verification Commands:**
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/strategies/shared/test_venue_policy.py -q`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/strategies/makerv3/test_order_safety.py -q -k 'borrow_cap or spot_borrow'`

**Step 1: Add the shared venue-policy tests**

Cover:

- exchange-code extraction
- borrow-cap reason detection
- non-PLUME symbols using the same reason parsing
- refusal to infer instrument family from symbol text

**Step 2: Implement the shared helper**

Move borrow-cap and exchange-code parsing into `shared/venue_policy.py`. Keep the helper pure and narrow, and return a structured result such as `reason_code`, `affected_side`, and `venue_code` rather than making later callers parse human text again.

**Step 3: Migrate Makerv3 onto the helper**

Replace direct parsing logic in `makerv3/failures.py` and related call sites with imports from the shared module.

**Step 4: Commit**

```bash
git add systems/flux/flux/strategies/shared/venue_policy.py \
  systems/flux/flux/strategies/makerv3/failures.py \
  systems/flux/flux/strategies/makerv3/strategy.py \
  tests/unit_tests/flux/strategies/shared/test_venue_policy.py
git commit -m "feat: add shared venue policy parsing"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 3: Align alerts, state exports, and API blocker semantics

**Files:**
- Modify: `systems/flux/flux/strategies/makerv3/strategy.py`
- Modify: `systems/flux/flux/strategies/makerv3/publisher.py`
- Modify: `systems/flux/flux/api/_payloads_signals.py`
- Modify: `tests/unit_tests/flux/api/test_payloads.py`
- Modify: `tests/unit_tests/flux/api/test_payload_snapshots.py`
- Modify: `systems/flux/docs/makerv3.md`

**Dependencies:** `Task 2: Extract shared venue-policy parsing and migrate Makerv3`

**Write Scope:** `systems/flux/flux/strategies/makerv3/strategy.py`, `systems/flux/flux/strategies/makerv3/publisher.py`, `systems/flux/flux/api/_payloads_signals.py`, `tests/unit_tests/flux/api/test_payloads.py`, `tests/unit_tests/flux/api/test_payload_snapshots.py`, `systems/flux/docs/makerv3.md`

**Verification Commands:**
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/strategies/makerv3/test_order_safety.py tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py -q`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/api/test_payloads.py -q -k 'quote_blockers or borrow'`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/api/test_payload_snapshots.py -q -k 'borrow_cap or quote_blockers'`

**Step 1: Implement the frozen contract**

Make implementation and docs match the contract above exactly. The alert path is actionable and cooldown-gated; this is not left open for interpretation.

**Step 2: Update state and API payload logic**

Ensure `quote_blockers` and related tradeability semantics are consistent from strategy state through API payload assembly.

**Step 3: Re-run focused tests**

Confirm the focused Makerv3 and API slices are green.

**Step 4: Commit**

```bash
git add systems/flux/flux/strategies/makerv3/strategy.py \
  systems/flux/flux/strategies/makerv3/publisher.py \
  systems/flux/flux/api/_payloads_signals.py \
  tests/unit_tests/flux/api/test_payloads.py \
  tests/unit_tests/flux/api/test_payload_snapshots.py \
  systems/flux/docs/makerv3.md
git commit -m "fix: align borrow cap alert and blocker contract"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 4: Run baseline verification and write rollback note

**Files:**
- Modify: `docs/plans/2026-03-31-strategy-platform-hardening-wave-pr0-baseline-safety-and-contract-freeze.md`
- Modify: `systems/flux/docs/makerv3.md`

**Dependencies:** `Task 3: Align alerts, state exports, and API blocker semantics`

**Write Scope:** `docs/plans/2026-03-31-strategy-platform-hardening-wave-pr0-baseline-safety-and-contract-freeze.md`, `systems/flux/docs/makerv3.md`

**Verification Commands:**
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/strategies/makerv3 -q`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/api/test_payloads.py tests/unit_tests/flux/api/test_payload_snapshots.py tests/unit_tests/flux/api/test_signals_inventory_contract.py tests/unit_tests/flux/runners/test_tokenmm_readiness.py tests/unit_tests/flux/bridge/test_handlers.py tests/unit_tests/flux/bridge/test_stream_consumer.py -q`
- `git diff --check`

**Step 1: Run the release bundle**

The Makerv3 suite plus the API/readiness/bridge contract slice must go green. This is the PR exit criterion.

**Step 2: Record rollback note**

Document that rollback is safe via whole-PR revert because no topic names, API routes, or payload field names changed.

**Step 3: Commit**

```bash
git add docs/plans/2026-03-31-strategy-platform-hardening-wave-pr0-baseline-safety-and-contract-freeze.md \
  systems/flux/docs/makerv3.md
git commit -m "docs: record pr0 verification and rollback"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.



---

## Source: `docs/plans/2026-03-31-strategy-platform-hardening-wave-pr1-shared-strategy-foundations.md`

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



---

## Source: `docs/plans/2026-03-31-strategy-platform-hardening-wave-pr2-shared-account-projection-ownership.md`

# Strategy Platform Hardening Wave PR2 Shared Account Projection Ownership Implementation Plan

> **For the execution agent:** REQUIRED SUB-SKILL: Before implementing this plan, choose exactly one execution mode and use the matching skill: `superpowers:subagent-driven-development` for same-session execution or `superpowers:executing-plans` for a separate-session handoff.
> **Progress:** The Progress Tracker in this document is the execution source of truth and must be updated on every state change.

**Goal:** Move shared-account projection row-selection helpers out of strategy-owned shared modules and into the common/read-model layer, move the runner-owned IBKR reference-balance provider into `flux.runners.shared`, and then migrate Makerv3, Makerv4, runner, and API consumers without changing behavior.

**Architecture:** Treat shared-account projection lookup as a read-model concern, not a strategy-platform concern. Create a common ownership module for account-projection position helpers, move the IBKR reference-balance provider and cache into `flux.runners.shared`, migrate Makerv3, Makerv4, runner, and API consumers onto those new owners, and delete the strategy-shared copies in the same PR.

**Tech Stack:** Python common/read-model code, Makerv3 and Makerv4 strategy code, profile-account tests, pytest.

**Context Docs:**
- Design: `docs/plans/2026-03-31-strategy-platform-hardening-wave-design.md`
- PRD: `docs/prd/2026-03-31-strategy-platform-hardening-wave.md`
- Relevant specs/runbooks: `docs/plans/2026-03-28-account-scoped-execution-controller-design.md`, `tests/unit_tests/flux/strategies/shared/test_account_projection_positions.py`

**Decision Summary:**
- projection row selection belongs outside `strategies/shared`
- this PR changes ownership only, not read-model semantics
- both Makerv3 and Makerv4 must migrate in the same PR
- the runner-owned IBKR reference-balance provider must stop living under `strategies/shared.equities_arb`
- no strategy-layer `reference_balances` re-export may survive this PR

## Release Gates

| Environment | Required for merge? | Requirement |
| --- | --- | --- |
| `default-unit` | yes | common/read-model, Makerv3, API, and runner proof must pass |
| `ibkr-unit` | yes | `python -c "import ibapi"` must succeed and Makerv4 proof must pass |
| `pilot` | yes | deploy one pinned pilot release from the PR head and validate the smoke bundle below |

## Affected Pilot Surfaces

- shared-account and profile-account consumers in Makerv3 and Makerv4
- runner profile-account surfaces
- API inventory and balances views that consume projection truth
- equities runner bootstrap and the moved IBKR reference-balance provider path

## Surface Contract Map

- `profile-account consumers`: `systems/flux/flux/runners/shared/profile_accounts.py` outputs and the balances/profile rows they feed
- `shared-account views`: strategy inventory state and projection-derived rows consumed by `/api/v1/signals`
- `inventory-facing API views`: `/api/v1/signals` and `/api/v1/balances` payloads that depend on shared-account row selection
- `reference-balance provider`: the IBKR snapshot provider currently rooted under `strategies/shared.equities_arb`

## Pilot Stacks To Move Together

- `tokenmm`
- `equities`
- `flux-api` if it is deployed as a separate pilot stack

## Pilot Deploy Units And Promotion Order

Deploy units:

- `flux.runners.tokenmm.run_node`
- `flux.runners.equities.run_node`
- `flux.api.app`

Promotion order:

1. build one pinned pilot release from the exact PR head
2. deploy `flux.runners.tokenmm.run_node`, `flux.runners.equities.run_node`, and `flux.api.app` from that same release
3. validate projection-derived views and runner-owned reference-balance bootstrap
4. promote the same units together only after the smoke bundle is clean

## Pilot Smoke Bundle

1. Deploy the pilot release from the exact PR head.
2. Confirm profile-account and shared-account views still resolve the same rows for live strategy surfaces.
3. Confirm tokenmm and Makerv4 inventory-facing views continue to publish the same projection-derived state.
4. Confirm the equities runner still initializes the moved runner-owned IBKR reference-balance provider and serves the expected balance source without importing deleted strategy-layer paths.
5. Confirm no strategy still imports the deleted `strategies/shared/account_projection_positions.py` path, and no runtime path still imports the deleted strategy-layer `reference_balances` modules.

## PR-Local Rollback

- revert the whole PR
- redeploy the previous pinned pilot release
- do not leave common projection helpers live while reverting only one consumer

## Progress Tracker

**Source of truth:** Update this table whenever task state changes. Do not rely on memory, chat history, or TodoWrite alone.

| Task | Status | Owner | Depends On | Write Scope | Lane Branch | Worktree Path | Commit / Diff | Verification | Notes / Last Update |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Overall | not_started | unassigned | none | `systems/flux/flux/common`, `systems/flux/flux/strategies/makerv3`, `systems/flux/flux/strategies/makerv4`, `systems/flux/flux/strategies/shared`, `tests/unit_tests/flux/common`, `tests/unit_tests/flux/strategies/makerv3`, `tests/unit_tests/flux/strategies/makerv4` | `wave/pr2-projection-ownership` | `.worktrees/strategy-platform-pr2` | none | not_run | Plan created |
| Task 1: Lock ownership and read-model behavior in tests | not_started | unassigned | none | `tests/unit_tests/flux/test_architecture_boundaries.py`, `tests/unit_tests/flux/strategies/shared/test_account_projection_positions.py`, `tests/unit_tests/flux/strategies/shared/test_equities_arb_core.py`, `tests/unit_tests/flux/strategies/makerv3/test_inventory.py`, `tests/unit_tests/flux/strategies/makerv4/test_inventory_contract.py`, `tests/unit_tests/flux/strategies/makerv4/test_reference_balances.py`, `tests/unit_tests/flux/runners/shared/test_profile_accounts.py`, `tests/unit_tests/flux/api/test_signals_inventory_contract.py`, `tests/unit_tests/flux/api/test_payloads.py` | `wave/pr2-projection-ownership` | `.worktrees/strategy-platform-pr2` | none | not_run | Plan created |
| Task 2: Create common account-projection position helper and runner-owned reference-balance provider | not_started | unassigned | Task 1: Lock ownership and read-model behavior in tests | `systems/flux/flux/common/account_projection_positions.py`, `systems/flux/flux/runners/shared/reference_balances.py`, `tests/unit_tests/flux/common/test_account_projection_positions.py`, `tests/unit_tests/flux/runners/shared/test_reference_balances.py`, `systems/flux/flux/strategies/shared/account_projection_positions.py`, `systems/flux/flux/strategies/shared/equities_arb/reference_balances.py` | `wave/pr2-projection-ownership` | `.worktrees/strategy-platform-pr2` | none | not_run | Plan created |
| Task 3: Migrate Makerv3, Makerv4, runner, and API consumers and delete the strategy-shared paths | not_started | unassigned | Task 2: Create common account-projection position helper and runner-owned reference-balance provider | `systems/flux/flux/strategies/makerv3/strategy.py`, `systems/flux/flux/strategies/makerv4/strategy.py`, `systems/flux/flux/runners/shared/profile_accounts.py`, `systems/flux/flux/runners/equities/run_node.py`, `systems/flux/flux/strategies/shared/equities_arb/__init__.py`, `systems/flux/flux/strategies/makerv4/reference_balances.py`, `tests/unit_tests/flux/strategies/shared/test_equities_arb_core.py`, `tests/unit_tests/flux/strategies/makerv3/test_inventory.py`, `tests/unit_tests/flux/strategies/makerv4/test_inventory_contract.py`, `tests/unit_tests/flux/strategies/makerv4/test_reference_balances.py`, `tests/unit_tests/flux/runners/shared/test_profile_accounts.py`, `tests/unit_tests/flux/api/test_signals_inventory_contract.py` | `wave/pr2-projection-ownership` | `.worktrees/strategy-platform-pr2` | none | not_run | Plan created |
| Task 4: Verify common/runner ownership and write rollback note | not_started | unassigned | Task 3: Migrate Makerv3, Makerv4, runner, and API consumers and delete the strategy-shared paths | `docs/plans/2026-03-31-strategy-platform-hardening-wave-pr2-shared-account-projection-ownership.md` | `wave/pr2-projection-ownership` | `.worktrees/strategy-platform-pr2` | none | not_run | Plan created |

---

### Task 1: Lock ownership and read-model behavior in tests

**Files:**
- Modify: `tests/unit_tests/flux/strategies/shared/test_account_projection_positions.py`
- Modify: `tests/unit_tests/flux/strategies/shared/test_equities_arb_core.py`
- Modify: `tests/unit_tests/flux/strategies/makerv3/test_inventory.py`
- Modify: `tests/unit_tests/flux/strategies/makerv4/test_inventory_contract.py`
- Modify: `tests/unit_tests/flux/strategies/makerv4/test_reference_balances.py`
- Modify: `tests/unit_tests/flux/runners/shared/test_profile_accounts.py`
- Modify: `tests/unit_tests/flux/api/test_signals_inventory_contract.py`
- Modify: `tests/unit_tests/flux/api/test_payloads.py`
- Modify: `tests/unit_tests/flux/test_architecture_boundaries.py`

**Dependencies:** `none`

**Write Scope:** `tests/unit_tests/flux/test_architecture_boundaries.py`, `tests/unit_tests/flux/strategies/shared/test_account_projection_positions.py`, `tests/unit_tests/flux/strategies/shared/test_equities_arb_core.py`, `tests/unit_tests/flux/strategies/makerv3/test_inventory.py`, `tests/unit_tests/flux/strategies/makerv4/test_inventory_contract.py`, `tests/unit_tests/flux/strategies/makerv4/test_reference_balances.py`, `tests/unit_tests/flux/runners/shared/test_profile_accounts.py`, `tests/unit_tests/flux/api/test_signals_inventory_contract.py`, `tests/unit_tests/flux/api/test_payloads.py`

**Verification Commands:**
- `python -c "import ibapi"`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/test_architecture_boundaries.py -q -k 'reference_balances or account_projection or runner_imports'`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/strategies/shared/test_account_projection_positions.py tests/unit_tests/flux/strategies/shared/test_equities_arb_core.py tests/unit_tests/flux/strategies/makerv3/test_inventory.py tests/unit_tests/flux/runners/shared/test_profile_accounts.py tests/unit_tests/flux/api/test_signals_inventory_contract.py tests/unit_tests/flux/api/test_payloads.py -q`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/strategies/makerv4/test_inventory_contract.py tests/unit_tests/flux/strategies/makerv4/test_reference_balances.py -q`

**Step 1: Write the failing tests**

Pin:

- row-selection semantics
- strategy consumer behavior
- runner profile-account and reference-balance behavior
- exact API inventory-facing surfaces that consume these rows, including `/api/v1/balances`
- the fact that ownership is changing, not the returned payload contract
- the architecture rule that strategies may not import the runner-owned reference-balance provider directly

**Step 2: Run tests to verify they fail**

Run the shared/common/Makerv3 slice in `default-unit` and the Makerv4 slice in `ibkr-unit`.

**Step 3: Commit**

```bash
git add tests/unit_tests/flux/strategies/shared/test_account_projection_positions.py \
  tests/unit_tests/flux/strategies/shared/test_equities_arb_core.py \
  tests/unit_tests/flux/strategies/makerv3/test_inventory.py \
  tests/unit_tests/flux/strategies/makerv4/test_inventory_contract.py \
  tests/unit_tests/flux/strategies/makerv4/test_reference_balances.py \
  tests/unit_tests/flux/runners/shared/test_profile_accounts.py \
  tests/unit_tests/flux/api/test_signals_inventory_contract.py \
  tests/unit_tests/flux/api/test_payloads.py \
  tests/unit_tests/flux/test_architecture_boundaries.py
git commit -m "test: lock account projection ownership contract"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 2: Create common account-projection position helper and runner-owned reference-balance provider

**Files:**
- Create: `systems/flux/flux/common/account_projection_positions.py`
- Create: `systems/flux/flux/runners/shared/reference_balances.py`
- Create: `tests/unit_tests/flux/common/test_account_projection_positions.py`
- Create: `tests/unit_tests/flux/runners/shared/test_reference_balances.py`
- Delete: `systems/flux/flux/strategies/shared/account_projection_positions.py`
- Delete: `systems/flux/flux/strategies/shared/equities_arb/reference_balances.py`

**Dependencies:** `Task 1: Lock ownership and read-model behavior in tests`

**Write Scope:** `systems/flux/flux/common/account_projection_positions.py`, `systems/flux/flux/runners/shared/reference_balances.py`, `tests/unit_tests/flux/common/test_account_projection_positions.py`, `tests/unit_tests/flux/runners/shared/test_reference_balances.py`, `systems/flux/flux/strategies/shared/account_projection_positions.py`, `systems/flux/flux/strategies/shared/equities_arb/reference_balances.py`

**Verification Commands:**
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/common/test_account_projection_positions.py tests/unit_tests/flux/runners/shared/test_reference_balances.py -q`

**Step 1: Copy the projection implementation into the common layer**

Keep behavior identical while moving the owner.

**Step 2: Move the runner-owned reference-balance provider**

Create a runner-owned home for the IBKR reference-balance provider and cache; do not leave that runtime dependency under `strategies/shared`.

**Step 3: Write or move unit tests**

The direct helper tests should now live under `tests/unit_tests/flux/common` and `tests/unit_tests/flux/runners/shared`.

**Step 4: Delete the old strategy-shared modules**

No compatibility wrapper. The old paths are removed in this PR.

**Step 5: Commit**

```bash
git add systems/flux/flux/common/account_projection_positions.py \
  systems/flux/flux/runners/shared/reference_balances.py \
  tests/unit_tests/flux/common/test_account_projection_positions.py \
  tests/unit_tests/flux/runners/shared/test_reference_balances.py \
  systems/flux/flux/strategies/shared/account_projection_positions.py \
  systems/flux/flux/strategies/shared/equities_arb/reference_balances.py
git commit -m "refactor: move projection and reference-balance helpers to proper owners"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 3: Migrate Makerv3, Makerv4, runner, and API consumers and delete the strategy-shared paths

**Files:**
- Modify: `systems/flux/flux/strategies/makerv3/strategy.py`
- Modify: `systems/flux/flux/strategies/makerv4/strategy.py`
- Modify: `systems/flux/flux/runners/shared/profile_accounts.py`
- Modify: `systems/flux/flux/runners/equities/run_node.py`
- Modify: `systems/flux/flux/strategies/shared/equities_arb/__init__.py`
- Delete: `systems/flux/flux/strategies/makerv4/reference_balances.py`
- Modify: `tests/unit_tests/flux/strategies/shared/test_equities_arb_core.py`
- Modify: `tests/unit_tests/flux/strategies/makerv3/test_inventory.py`
- Modify: `tests/unit_tests/flux/strategies/makerv4/test_inventory_contract.py`
- Modify: `tests/unit_tests/flux/strategies/makerv4/test_reference_balances.py`
- Modify: `tests/unit_tests/flux/runners/shared/test_profile_accounts.py`
- Modify: `tests/unit_tests/flux/api/test_signals_inventory_contract.py`
- Modify: `tests/unit_tests/flux/api/test_payloads.py`
- Modify: `tests/unit_tests/flux/test_architecture_boundaries.py`

**Dependencies:** `Task 2: Create common account-projection position helper and runner-owned reference-balance provider`

**Write Scope:** `systems/flux/flux/strategies/makerv3/strategy.py`, `systems/flux/flux/strategies/makerv4/strategy.py`, `systems/flux/flux/runners/shared/profile_accounts.py`, `systems/flux/flux/runners/equities/run_node.py`, `systems/flux/flux/strategies/shared/equities_arb/__init__.py`, `systems/flux/flux/strategies/makerv4/reference_balances.py`, `tests/unit_tests/flux/strategies/shared/test_equities_arb_core.py`, `tests/unit_tests/flux/strategies/makerv3/test_inventory.py`, `tests/unit_tests/flux/strategies/makerv4/test_inventory_contract.py`, `tests/unit_tests/flux/strategies/makerv4/test_reference_balances.py`, `tests/unit_tests/flux/runners/shared/test_profile_accounts.py`, `tests/unit_tests/flux/api/test_signals_inventory_contract.py`, `tests/unit_tests/flux/api/test_payloads.py`, `tests/unit_tests/flux/test_architecture_boundaries.py`

**Verification Commands:**
- `python -c "import ibapi"`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/test_architecture_boundaries.py -q -k 'reference_balances or account_projection or runner_imports'`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/strategies/shared/test_equities_arb_core.py tests/unit_tests/flux/strategies/makerv3/test_inventory.py tests/unit_tests/flux/runners/shared/test_profile_accounts.py tests/unit_tests/flux/api/test_signals_inventory_contract.py tests/unit_tests/flux/api/test_payloads.py -q`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/strategies/makerv4/test_inventory_contract.py tests/unit_tests/flux/strategies/makerv4/test_reference_balances.py -q`
- `rg -n "account_projection_positions|reference_balances" systems/flux/flux/strategies/{makerv3,makerv4,shared} systems/flux/flux/runners/{shared,equities}`

**Step 1: Update imports in Makerv3, Makerv4, runner, and API consumers**

Point both strategy families, runner consumers, and API-facing proof surfaces at the new common/runner owners. Delete the Makerv4 strategy-layer `reference_balances` surface rather than turning it into a runner re-export.

**Step 2: Re-run the strategy tests**

Confirm no payload or behavior regression. Run the Makerv3 slice in `default-unit` and the Makerv4 slice in `ibkr-unit`.

**Step 3: Verify the old paths are gone and the runner boundary is enforced**

The ripgrep check must show no remaining production dependency on the deleted strategy-shared module paths and no surviving strategy-layer `reference_balances` re-export. The architecture-boundary test must encode that strategies do not import the runner-owned provider directly.

**Step 4: Commit**

```bash
git add systems/flux/flux/strategies/makerv3/strategy.py \
  systems/flux/flux/strategies/makerv4/strategy.py \
  systems/flux/flux/runners/shared/profile_accounts.py \
  systems/flux/flux/runners/equities/run_node.py \
  systems/flux/flux/strategies/shared/equities_arb/__init__.py \
  systems/flux/flux/strategies/makerv4/reference_balances.py \
  tests/unit_tests/flux/strategies/shared/test_equities_arb_core.py \
  tests/unit_tests/flux/strategies/makerv3/test_inventory.py \
  tests/unit_tests/flux/strategies/makerv4/test_inventory_contract.py \
  tests/unit_tests/flux/strategies/makerv4/test_reference_balances.py \
  tests/unit_tests/flux/runners/shared/test_profile_accounts.py \
  tests/unit_tests/flux/api/test_signals_inventory_contract.py \
  tests/unit_tests/flux/api/test_payloads.py \
  tests/unit_tests/flux/test_architecture_boundaries.py
git commit -m "refactor: migrate consumers to common and runner ownership"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 4: Verify common/runner ownership and write rollback note

**Files:**
- Modify: `docs/plans/2026-03-31-strategy-platform-hardening-wave-pr2-shared-account-projection-ownership.md`

**Dependencies:** `Task 3: Migrate Makerv3, Makerv4, runner, and API consumers and delete the strategy-shared paths`

**Write Scope:** `docs/plans/2026-03-31-strategy-platform-hardening-wave-pr2-shared-account-projection-ownership.md`

**Verification Commands:**
- `python -c "import ibapi"`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/test_architecture_boundaries.py -q -k 'reference_balances or account_projection or runner_imports'`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/common/test_account_projection_positions.py tests/unit_tests/flux/strategies/shared/test_equities_arb_core.py tests/unit_tests/flux/runners/shared/test_profile_accounts.py tests/unit_tests/flux/api/test_signals_inventory_contract.py tests/unit_tests/flux/api/test_payloads.py tests/unit_tests/flux/strategies/makerv3/test_inventory.py -q`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/strategies/makerv4/test_inventory_contract.py tests/unit_tests/flux/strategies/makerv4/test_reference_balances.py tests/unit_tests/flux/runners/shared/test_reference_balances.py -q`
- `git diff --check`

**Step 1: Run the combined verification bundle**

This PR is done only when the common helper and both strategy consumers are green together.

**Step 2: Record rollback note**

State explicitly that rollback is safe because the helper moved but public strategy/API contracts did not.

**Step 3: Commit**

```bash
git add docs/plans/2026-03-31-strategy-platform-hardening-wave-pr2-shared-account-projection-ownership.md
git commit -m "docs: record pr2 verification and rollback"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.



---

## Source: `docs/plans/2026-03-31-strategy-platform-hardening-wave-pr3-shared-observability-platform.md`

# Strategy Platform Hardening Wave PR3 Shared Observability Platform Implementation Plan

> **For the execution agent:** REQUIRED SUB-SKILL: Before implementing this plan, choose exactly one execution mode and use the matching skill: `superpowers:subagent-driven-development` for same-session execution or `superpowers:executing-plans` for a separate-session handoff.
> **Progress:** The Progress Tracker in this document is the execution source of truth and must be updated on every state change.

**Goal:** Move strategy-generic observability helpers out of Makerv3 ownership, migrate all current consumers, and preserve topic names plus operator/API payload semantics through an explicit shared observability-contract namespace.

**Architecture:** Extract generic serialization, legacy topic constants, payload-schema helpers, alert publishing, JSON/event publishing, balance/trade helper logic, and related helpers into an explicit `strategies/shared/observability` ownership surface. Keep family-specific state builders local. This PR must remove the current `shared -> makerv3` and `makerv4 -> makerv3 publisher/constants` dependency edges without renaming external topics.

**Tech Stack:** Python strategy/runtime publishing code, shared helpers, API payload builders, pytest.

**Context Docs:**
- Design: `docs/plans/2026-03-31-strategy-platform-hardening-wave-design.md`
- PRD: `docs/prd/2026-03-31-strategy-platform-hardening-wave.md`
- Relevant specs/runbooks: `systems/flux/docs/makerv3.md`, `docs/plans/2026-03-31-strategy-platform-hardening-wave.md`

**Decision Summary:**
- preserve all current topic names in this wave
- extract helper ownership, not family-specific state construction
- migrate Makerv3, Makerv4, equities, and `shared/*` consumers in the same PR
- frozen external topic and payload-schema ownership should live under `observability/contracts`, not a generic top-level helper bucket

## Release Gates

| Environment | Required for merge? | Requirement |
| --- | --- | --- |
| `default-unit` | yes | shared, Makerv3, API payload, and realtime proof must pass |
| `ibkr-unit` | yes | `python -c "import ibapi"` must succeed and Makerv4 proof must pass |
| `pilot` | yes | deploy one pinned pilot release from the PR head and validate the smoke bundle below |

## Affected Pilot Surfaces

- Makerv3 and Makerv4 state, alert, trade, and balances publishers
- shared equities helper modules that publish observability payloads
- realtime, signals, and balances API consumers
- readiness and bridge consumers of frozen legacy topics

## Pilot Stacks To Move Together

- `tokenmm`
- `equities`
- `flux-api` if it is deployed as a separate pilot stack

## Pilot Deploy Units And Promotion Order

Deploy units:

- `flux.runners.tokenmm.run_node`
- `flux.runners.tokenmm.run_bridge`
- `flux.runners.equities.run_node`
- `flux.runners.equities.run_bridge`
- `flux.api.app`

Promotion order:

1. build one pinned pilot release from the exact PR head
2. deploy the tokenmm and equities runner/bridge units plus `flux.api.app` from that same release
3. validate frozen topic routing, payload decoding, and observability bootstrap
4. promote the same units together only after the smoke bundle is clean

## Pilot Smoke Bundle

1. Deploy the pilot release from the exact PR head.
2. Confirm legacy topic names remain unchanged for state, alert, balances, order-intent, and trade paths.
3. Confirm realtime and API payload consumers still decode the same keys and semantics.
4. Confirm Makerv4 and shared equities helpers no longer depend on MakerV3 publisher/constants paths.

## PR-Local Rollback

- revert the whole PR
- redeploy the previous pinned pilot release
- do not leave shared observability helpers live while reverting only one publishing family

## Progress Tracker

**Source of truth:** Update this table whenever task state changes. Do not rely on memory, chat history, or TodoWrite alone.

| Task | Status | Owner | Depends On | Write Scope | Lane Branch | Worktree Path | Commit / Diff | Verification | Notes / Last Update |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Overall | not_started | unassigned | none | `systems/flux/flux/strategies/shared`, `systems/flux/flux/strategies/makerv3`, `systems/flux/flux/strategies/makerv4`, `systems/flux/flux/strategies/equities_taker`, `systems/flux/flux/api`, `tests/unit_tests/flux/strategies`, `tests/unit_tests/flux/api`, `systems/flux/docs` | `wave/pr3-observability-platform` | `.worktrees/strategy-platform-pr3` | none | not_run | Plan created |
| Task 1: Lock topic and payload contracts in tests | not_started | unassigned | none | `tests/unit_tests/flux/test_architecture_boundaries.py`, `tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py`, `tests/unit_tests/flux/strategies/makerv4/test_observability_and_exports.py`, `tests/unit_tests/flux/strategies/shared/test_trades.py`, `tests/unit_tests/flux/strategies/shared/test_equities_arb_core.py`, `tests/unit_tests/flux/api/test_payloads.py`, `tests/unit_tests/flux/api/test_payload_snapshots.py`, `tests/unit_tests/flux/api/test_realtime_contract.py`, `tests/unit_tests/flux/runners/test_tokenmm_readiness.py`, `tests/unit_tests/examples/strategies/test_tokenmm_run_bridge.py`, `tests/unit_tests/examples/strategies/test_equities_run_bridge.py` | `wave/pr3-observability-platform` | `.worktrees/strategy-platform-pr3` | none | not_run | Plan created |
| Task 2: Extract shared observability modules | not_started | unassigned | Task 1: Lock topic and payload contracts in tests | `systems/flux/flux/strategies/shared/observability/contracts.py`, `systems/flux/flux/strategies/shared/observability/serialization.py`, `systems/flux/flux/strategies/shared/observability/publisher.py`, `systems/flux/flux/strategies/shared/alerts.py`, `systems/flux/flux/strategies/shared/trades.py`, `tests/unit_tests/flux/strategies/shared/test_trades.py`, `tests/unit_tests/flux/test_architecture_boundaries.py` | `wave/pr3-observability-platform` | `.worktrees/strategy-platform-pr3` | none | not_run | Plan created |
| Task 3: Migrate all strategy, runner, and shared consumers, delete upward imports | not_started | unassigned | Task 2: Extract shared observability modules | `systems/flux/flux/strategies/makerv3/publisher.py`, `systems/flux/flux/strategies/makerv4/strategy.py`, `systems/flux/flux/strategies/shared/equities_arb/hedging.py`, `systems/flux/flux/strategies/shared/alerts.py`, `systems/flux/flux/runners/tokenmm/readiness.py`, `systems/flux/flux/runners/tokenmm/run_bridge.py`, `systems/flux/flux/runners/tokenmm/run_node.py`, `systems/flux/flux/runners/equities/run_bridge.py`, `systems/flux/flux/runners/equities/run_node.py`, `tests/unit_tests/flux/strategies/makerv4/test_publisher_contract.py`, `tests/unit_tests/flux/strategies/makerv4/test_observability_and_exports.py`, `tests/unit_tests/flux/runners/test_tokenmm_readiness.py`, `tests/unit_tests/examples/strategies/test_tokenmm_run_bridge.py`, `tests/unit_tests/examples/strategies/test_equities_run_bridge.py`, `tests/unit_tests/flux/api/test_payload_snapshots.py`, `tests/unit_tests/flux/test_architecture_boundaries.py` | `wave/pr3-observability-platform` | `.worktrees/strategy-platform-pr3` | none | not_run | Plan created |
| Task 4: Verify payload preservation and record rollback note | not_started | unassigned | Task 3: Migrate all strategy and shared consumers, delete upward imports | `systems/flux/docs/makerv3.md`, `docs/plans/2026-03-31-strategy-platform-hardening-wave-pr3-shared-observability-platform.md` | `wave/pr3-observability-platform` | `.worktrees/strategy-platform-pr3` | none | not_run | Plan created |

---

### Task 1: Lock topic and payload contracts in tests

**Files:**
- Modify: `tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py`
- Modify: `tests/unit_tests/flux/strategies/makerv4/test_observability_and_exports.py`
- Modify: `tests/unit_tests/flux/strategies/shared/test_trades.py`
- Modify: `tests/unit_tests/flux/strategies/shared/test_equities_arb_core.py`
- Modify: `tests/unit_tests/flux/api/test_payloads.py`
- Create: `tests/unit_tests/flux/api/test_payload_snapshots.py`
- Modify: `tests/unit_tests/flux/api/test_realtime_contract.py`
- Modify: `tests/unit_tests/flux/runners/test_tokenmm_readiness.py`
- Modify: `tests/unit_tests/examples/strategies/test_tokenmm_run_bridge.py`
- Modify: `tests/unit_tests/examples/strategies/test_equities_run_bridge.py`
- Modify: `tests/unit_tests/flux/test_architecture_boundaries.py`

**Dependencies:** `none`

**Write Scope:** `tests/unit_tests/flux/test_architecture_boundaries.py`, `tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py`, `tests/unit_tests/flux/strategies/makerv4/test_observability_and_exports.py`, `tests/unit_tests/flux/strategies/shared/test_trades.py`, `tests/unit_tests/flux/strategies/shared/test_equities_arb_core.py`, `tests/unit_tests/flux/api/test_payloads.py`, `tests/unit_tests/flux/api/test_payload_snapshots.py`, `tests/unit_tests/flux/api/test_realtime_contract.py`, `tests/unit_tests/flux/runners/test_tokenmm_readiness.py`, `tests/unit_tests/examples/strategies/test_tokenmm_run_bridge.py`, `tests/unit_tests/examples/strategies/test_equities_run_bridge.py`

**Verification Commands:**
- `python -c "import ibapi"`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/test_architecture_boundaries.py tests/unit_tests/flux/strategies/shared/test_trades.py tests/unit_tests/flux/strategies/shared/test_equities_arb_core.py tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py tests/unit_tests/flux/api/test_payloads.py tests/unit_tests/flux/api/test_payload_snapshots.py tests/unit_tests/flux/api/test_realtime_contract.py tests/unit_tests/flux/runners/test_tokenmm_readiness.py tests/unit_tests/examples/strategies/test_tokenmm_run_bridge.py tests/unit_tests/examples/strategies/test_equities_run_bridge.py -q`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/strategies/makerv4/test_observability_and_exports.py -q`

**Step 1: Write the failing tests**

Pin:

- topic names
- payload keys and semantics
- golden payload fixtures for representative alert/event/balances/order-intent/trade envelopes
- trade quantity contract
- strategy state/event export semantics
- readiness and bridge routing semantics for the frozen legacy topics

**Step 2: Run tests to verify they fail**

Run the shared/Makerv3/API slice in `default-unit` and the Makerv4 slice in `ibkr-unit`.

**Step 3: Commit**

```bash
git add tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py \
  tests/unit_tests/flux/strategies/makerv4/test_observability_and_exports.py \
  tests/unit_tests/flux/strategies/shared/test_trades.py \
  tests/unit_tests/flux/strategies/shared/test_equities_arb_core.py \
  tests/unit_tests/flux/api/test_payloads.py \
  tests/unit_tests/flux/api/test_payload_snapshots.py \
  tests/unit_tests/flux/api/test_realtime_contract.py \
  tests/unit_tests/flux/runners/test_tokenmm_readiness.py \
  tests/unit_tests/examples/strategies/test_tokenmm_run_bridge.py \
  tests/unit_tests/examples/strategies/test_equities_run_bridge.py \
  tests/unit_tests/flux/test_architecture_boundaries.py
git commit -m "test: lock observability platform contracts"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 2: Extract shared observability modules

**Files:**
- Create: `systems/flux/flux/strategies/shared/observability/contracts.py`
- Create: `systems/flux/flux/strategies/shared/observability/serialization.py`
- Create: `systems/flux/flux/strategies/shared/observability/publisher.py`
- Modify: `systems/flux/flux/strategies/shared/alerts.py`
- Modify: `systems/flux/flux/strategies/shared/trades.py`
- Modify: `tests/unit_tests/flux/strategies/shared/test_trades.py`
- Modify: `tests/unit_tests/flux/test_architecture_boundaries.py`

**Dependencies:** `Task 1: Lock topic and payload contracts in tests`

**Write Scope:** `systems/flux/flux/strategies/shared/observability/contracts.py`, `systems/flux/flux/strategies/shared/observability/serialization.py`, `systems/flux/flux/strategies/shared/observability/publisher.py`, `systems/flux/flux/strategies/shared/alerts.py`, `systems/flux/flux/strategies/shared/trades.py`, `tests/unit_tests/flux/strategies/shared/test_trades.py`, `tests/unit_tests/flux/test_architecture_boundaries.py`

**Verification Commands:**
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/test_architecture_boundaries.py tests/unit_tests/flux/strategies/shared/test_trades.py -q -k 'architecture or trade'`

**Step 1: Add the new shared helper modules**

Move generic:

- topic constants and frozen payload-schema helpers under `observability/contracts`
- decimal/json serialization
- generic publish helpers
- alert helper logic

into shared ownership.

**Step 2: Keep external contracts stable**

The helper modules may move, but topic names and payload field names must remain unchanged.

**Step 3: Commit**

```bash
git add systems/flux/flux/strategies/shared/observability/contracts.py \
  systems/flux/flux/strategies/shared/observability/serialization.py \
  systems/flux/flux/strategies/shared/observability/publisher.py \
  systems/flux/flux/strategies/shared/alerts.py \
  systems/flux/flux/strategies/shared/trades.py \
  tests/unit_tests/flux/strategies/shared/test_trades.py \
  tests/unit_tests/flux/test_architecture_boundaries.py
git commit -m "feat: extract shared observability helpers"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 3: Migrate all strategy, runner, and shared consumers, delete upward imports

**Files:**
- Modify: `systems/flux/flux/strategies/makerv3/publisher.py`
- Modify: `systems/flux/flux/strategies/makerv4/strategy.py`
- Modify: `systems/flux/flux/strategies/shared/equities_arb/hedging.py`
- Modify: `systems/flux/flux/strategies/shared/alerts.py`
- Modify: `systems/flux/flux/runners/tokenmm/readiness.py`
- Modify: `systems/flux/flux/runners/tokenmm/run_bridge.py`
- Modify: `systems/flux/flux/runners/tokenmm/run_node.py`
- Modify: `systems/flux/flux/runners/equities/run_bridge.py`
- Modify: `systems/flux/flux/runners/equities/run_node.py`
- Modify: `tests/unit_tests/flux/strategies/makerv4/test_publisher_contract.py`
- Modify: `tests/unit_tests/flux/strategies/makerv4/test_observability_and_exports.py`
- Modify: `tests/unit_tests/flux/runners/test_tokenmm_readiness.py`
- Modify: `tests/unit_tests/examples/strategies/test_tokenmm_run_bridge.py`
- Modify: `tests/unit_tests/examples/strategies/test_equities_run_bridge.py`
- Modify: `tests/unit_tests/flux/api/test_payload_snapshots.py`
- Modify: `tests/unit_tests/flux/test_architecture_boundaries.py`

**Dependencies:** `Task 2: Extract shared observability modules`

**Write Scope:** `systems/flux/flux/strategies/makerv3/publisher.py`, `systems/flux/flux/strategies/makerv4/strategy.py`, `systems/flux/flux/strategies/shared/equities_arb/hedging.py`, `systems/flux/flux/strategies/shared/alerts.py`, `systems/flux/flux/runners/tokenmm/readiness.py`, `systems/flux/flux/runners/tokenmm/run_bridge.py`, `systems/flux/flux/runners/tokenmm/run_node.py`, `systems/flux/flux/runners/equities/run_bridge.py`, `systems/flux/flux/runners/equities/run_node.py`, `tests/unit_tests/flux/strategies/makerv4/test_publisher_contract.py`, `tests/unit_tests/flux/strategies/makerv4/test_observability_and_exports.py`, `tests/unit_tests/flux/runners/test_tokenmm_readiness.py`, `tests/unit_tests/examples/strategies/test_tokenmm_run_bridge.py`, `tests/unit_tests/examples/strategies/test_equities_run_bridge.py`, `tests/unit_tests/flux/api/test_payload_snapshots.py`, `tests/unit_tests/flux/test_architecture_boundaries.py`

**Verification Commands:**
- `python -c "import ibapi"`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/strategies/makerv4/test_publisher_contract.py tests/unit_tests/flux/strategies/makerv4/test_observability_and_exports.py -q`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/runners/test_tokenmm_readiness.py tests/unit_tests/examples/strategies/test_tokenmm_run_bridge.py tests/unit_tests/examples/strategies/test_equities_run_bridge.py tests/unit_tests/flux/api/test_payload_snapshots.py tests/unit_tests/flux/test_architecture_boundaries.py -q`
- `rg -n "from flux\\.strategies\\.makerv3|import .*makerv3" systems/flux/flux/strategies/{makerv4,shared} systems/flux/flux/runners/{tokenmm,equities}`

**Step 1: Migrate Makerv4, runner, and shared-family consumers**

Point Makerv4, bridge/readiness consumers, and `shared/*` modules at the new shared observability helpers.

**Step 2: Slim Makerv3 publisher**

Keep only family-specific state-building logic in `makerv3/publisher.py`; shared low-level helpers should now import from `strategies/shared`.

**Step 3: Verify upward imports are gone across strategy and runner consumers**

The ripgrep command must show no Makerv3 publisher/constants imports in Makerv4, `shared/*`, or the runner bridge/readiness modules migrated in this PR.

**Step 4: Commit**

```bash
git add systems/flux/flux/strategies/makerv3/publisher.py \
  systems/flux/flux/strategies/makerv4/strategy.py \
  systems/flux/flux/strategies/shared/equities_arb/hedging.py \
  systems/flux/flux/strategies/shared/alerts.py \
  tests/unit_tests/flux/strategies/makerv4/test_publisher_contract.py \
  tests/unit_tests/flux/strategies/makerv4/test_observability_and_exports.py \
  tests/unit_tests/flux/api/test_payload_snapshots.py \
  tests/unit_tests/flux/test_architecture_boundaries.py
git commit -m "refactor: migrate consumers to shared observability platform"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 4: Verify payload preservation and record rollback note

**Files:**
- Modify: `systems/flux/docs/makerv3.md`
- Modify: `docs/plans/2026-03-31-strategy-platform-hardening-wave-pr3-shared-observability-platform.md`

**Dependencies:** `Task 3: Migrate all strategy and shared consumers, delete upward imports`

**Write Scope:** `systems/flux/docs/makerv3.md`, `docs/plans/2026-03-31-strategy-platform-hardening-wave-pr3-shared-observability-platform.md`

**Verification Commands:**
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/test_architecture_boundaries.py tests/unit_tests/flux/strategies/shared/test_trades.py tests/unit_tests/flux/strategies/shared/test_equities_arb_core.py tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py tests/unit_tests/flux/api/test_payloads.py tests/unit_tests/flux/api/test_payload_snapshots.py tests/unit_tests/flux/api/test_realtime_contract.py tests/unit_tests/flux/runners/test_tokenmm_readiness.py tests/unit_tests/examples/strategies/test_tokenmm_run_bridge.py tests/unit_tests/examples/strategies/test_equities_run_bridge.py -q`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/strategies/makerv4/test_observability_and_exports.py tests/unit_tests/flux/strategies/makerv4/test_publisher_contract.py -q`
- `git diff --check`

**Step 1: Run the combined bundle**

The shared helper tests, strategy export tests, runner bridge/readiness tests, and API payload tests must pass together.

**Step 2: Update docs**

State explicitly that helper ownership changed while topic names and payload keys remained frozen.

**Step 3: Commit**

```bash
git add systems/flux/docs/makerv3.md \
  docs/plans/2026-03-31-strategy-platform-hardening-wave-pr3-shared-observability-platform.md
git commit -m "docs: record pr3 verification and rollback"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.



---

## Source: `docs/plans/2026-03-31-strategy-platform-hardening-wave-pr4-shared-execution-primitives.md`

# Strategy Platform Hardening Wave PR4 Shared Execution Primitives Implementation Plan

> **For the execution agent:** REQUIRED SUB-SKILL: Before implementing this plan, choose exactly one execution mode and use the matching skill: `superpowers:subagent-driven-development` for same-session execution or `superpowers:executing-plans` for a separate-session handoff.
> **Progress:** The Progress Tracker in this document is the execution source of truth and must be updated on every state change.

**Goal:** Move reusable managed-order helpers and strategy-facing inventory/exposure adapters out of Makerv3 ownership, migrate all current consumers, and preserve external inventory and order-safety behavior.

**Architecture:** Extract only the genuinely shared execution primitives. Managed-order collection and order-lineage helpers move to shared ownership. Strategy-facing inventory adapters move to shared ownership, but raw quantity normalization and projection truth remain in `flux.common` and do not move here because `PR2` already corrected projection ownership. This PR consumes the first-class typed market-identity contract introduced in `PR1`; it does not invent a new normalization layer. Family-local skew policy and orchestration remain local.

**Tech Stack:** Python strategy/runtime code, shared helpers, API inventory payload tests, pytest.

**Context Docs:**
- Design: `docs/plans/2026-03-31-strategy-platform-hardening-wave-design.md`
- PRD: `docs/prd/2026-03-31-strategy-platform-hardening-wave.md`
- Relevant specs/runbooks: `systems/flux/docs/makerv3.md`, `docs/plans/2026-03-31-strategy-platform-hardening-wave.md`

**Decision Summary:**
- only reusable execution primitives move
- shared-account read-model ownership is explicitly out of scope for this PR because it already moved in `PR2`
- Makerv3 and Makerv4 migrate in the same PR so no shared primitive remains dual-owned
- this PR must replace duplicated Makerv3/Makerv4 market-type heuristics in the migrated execution path with the common normalized inputs defined earlier in the wave

## Release Gates

| Environment | Required for merge? | Requirement |
| --- | --- | --- |
| `default-unit` | yes | shared, Makerv3, and API inventory/order-safety proof must pass |
| `ibkr-unit` | yes | `python -c "import ibapi"` must succeed and Makerv4 proof must pass |
| `pilot` | yes | deploy one pinned pilot release from the PR head and validate the smoke bundle below |

## Affected Pilot Surfaces

- Makerv3 and Makerv4 managed-order and cancel-safety behavior
- strategy inventory and exposure exports
- `/api/v1/signals` inventory-facing consumers

## Pilot Stacks To Move Together

- `tokenmm`
- `equities`
- `flux-api` if it is deployed as a separate pilot stack

## Pilot Deploy Units And Promotion Order

Deploy units:

- `flux.runners.tokenmm.run_node`
- `flux.runners.equities.run_node`
- `flux.api.app`

Promotion order:

1. build one pinned pilot release from the exact PR head
2. deploy `flux.runners.tokenmm.run_node`, `flux.runners.equities.run_node`, and `flux.api.app` from that same release
3. validate managed-order flow, inventory exports, and order-intent linkage
4. promote the same units together only after the smoke bundle is clean

## Pilot Smoke Bundle

1. Deploy the pilot release from the exact PR head.
2. Confirm Makerv3 and Makerv4 still publish the same inventory-facing state and order-intent linkage semantics.
3. Confirm cancel-safety, reconciliation, and managed-order flows remain stable under normal quote maintenance.
4. Confirm Makerv4 no longer imports MakerV3 managed-order or inventory helpers.

## PR-Local Rollback

- revert the whole PR
- redeploy the previous pinned pilot release
- do not leave shared execution helpers live while reverting only one consumer

## Progress Tracker

**Source of truth:** Update this table whenever task state changes. Do not rely on memory, chat history, or TodoWrite alone.

| Task | Status | Owner | Depends On | Write Scope | Lane Branch | Worktree Path | Commit / Diff | Verification | Notes / Last Update |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Overall | not_started | unassigned | none | `systems/flux/flux/strategies/shared`, `systems/flux/flux/strategies/makerv3`, `systems/flux/flux/strategies/makerv4`, `systems/flux/flux/api`, `tests/unit_tests/flux/strategies/shared`, `tests/unit_tests/flux/strategies/makerv3`, `tests/unit_tests/flux/strategies/makerv4`, `tests/unit_tests/flux/api` | `wave/pr4-execution-primitives` | `.worktrees/strategy-platform-pr4` | none | not_run | Plan created |
| Task 1: Lock shared managed-order and inventory contracts in tests | not_started | unassigned | none | `tests/unit_tests/flux/test_architecture_boundaries.py`, `tests/unit_tests/flux/common/test_market_identity.py`, `tests/unit_tests/flux/common/test_strategy_contracts.py`, `tests/unit_tests/flux/strategies/makerv3/test_managed_orders.py`, `tests/unit_tests/flux/strategies/makerv3/test_inventory.py`, `tests/unit_tests/flux/strategies/makerv4/test_inventory_contract.py`, `tests/unit_tests/flux/strategies/makerv4/test_strategy.py`, `tests/unit_tests/flux/strategies/makerv4/test_instruments.py`, `tests/unit_tests/flux/api/test_signals_inventory_contract.py`, `tests/unit_tests/flux/api/test_payload_snapshots.py` | `wave/pr4-execution-primitives` | `.worktrees/strategy-platform-pr4` | none | not_run | Plan created |
| Task 2: Extract shared managed-order helpers and migrate consumers | not_started | unassigned | Task 1: Lock shared managed-order and inventory contracts in tests | `systems/flux/flux/strategies/shared/managed_orders.py`, `systems/flux/flux/strategies/makerv3/managed_orders.py`, `systems/flux/flux/strategies/makerv4/strategy.py`, `tests/unit_tests/flux/strategies/shared/test_managed_orders.py`, `tests/unit_tests/flux/strategies/makerv3/test_managed_orders.py` | `wave/pr4-execution-primitives` | `.worktrees/strategy-platform-pr4` | none | not_run | Plan created |
| Task 3: Extract shared inventory adapters and migrate consumers | not_started | unassigned | Task 2: Extract shared managed-order helpers and migrate consumers | `systems/flux/flux/common/strategy_contracts.py`, `systems/flux/flux/strategies/shared/inventory_math.py`, `systems/flux/flux/strategies/makerv3/inventory.py`, `systems/flux/flux/strategies/makerv3/strategy.py`, `systems/flux/flux/strategies/makerv4/strategy.py`, `tests/unit_tests/flux/common/test_strategy_contracts.py`, `tests/unit_tests/flux/strategies/shared/test_inventory_math.py`, `tests/unit_tests/flux/strategies/makerv3/test_inventory.py`, `tests/unit_tests/flux/strategies/makerv4/test_inventory_contract.py`, `tests/unit_tests/flux/strategies/makerv4/test_instruments.py`, `tests/unit_tests/flux/api/test_signals_inventory_contract.py` | `wave/pr4-execution-primitives` | `.worktrees/strategy-platform-pr4` | none | not_run | Plan created |
| Task 4: Verify import cleanup and record rollback note | not_started | unassigned | Task 3: Extract shared inventory adapters and migrate consumers | `systems/flux/docs/makerv3.md`, `docs/plans/2026-03-31-strategy-platform-hardening-wave-pr4-shared-execution-primitives.md` | `wave/pr4-execution-primitives` | `.worktrees/strategy-platform-pr4` | none | not_run | Plan created |

---

### Task 1: Lock shared managed-order and inventory contracts in tests

**Files:**
- Modify: `tests/unit_tests/flux/common/test_strategy_contracts.py`
- Modify: `tests/unit_tests/flux/strategies/makerv3/test_managed_orders.py`
- Modify: `tests/unit_tests/flux/strategies/makerv3/test_inventory.py`
- Modify: `tests/unit_tests/flux/strategies/makerv4/test_inventory_contract.py`
- Modify: `tests/unit_tests/flux/strategies/makerv4/test_strategy.py`
- Modify: `tests/unit_tests/flux/strategies/makerv4/test_instruments.py`
- Modify: `tests/unit_tests/flux/api/test_signals_inventory_contract.py`
- Modify: `tests/unit_tests/flux/api/test_payload_snapshots.py`
- Modify: `tests/unit_tests/flux/test_architecture_boundaries.py`
- Modify: `tests/unit_tests/flux/common/test_market_identity.py`

**Dependencies:** `none`

**Write Scope:** `tests/unit_tests/flux/test_architecture_boundaries.py`, `tests/unit_tests/flux/common/test_market_identity.py`, `tests/unit_tests/flux/common/test_strategy_contracts.py`, `tests/unit_tests/flux/strategies/makerv3/test_managed_orders.py`, `tests/unit_tests/flux/strategies/makerv3/test_inventory.py`, `tests/unit_tests/flux/strategies/makerv4/test_inventory_contract.py`, `tests/unit_tests/flux/strategies/makerv4/test_strategy.py`, `tests/unit_tests/flux/strategies/makerv4/test_instruments.py`, `tests/unit_tests/flux/api/test_signals_inventory_contract.py`, `tests/unit_tests/flux/api/test_payload_snapshots.py`

**Verification Commands:**
- `python -c "import ibapi"`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/test_architecture_boundaries.py tests/unit_tests/flux/common/test_market_identity.py tests/unit_tests/flux/common/test_strategy_contracts.py tests/unit_tests/flux/strategies/makerv3/test_managed_orders.py tests/unit_tests/flux/strategies/makerv3/test_inventory.py tests/unit_tests/flux/strategies/makerv3/test_order_safety.py tests/unit_tests/flux/strategies/makerv3/test_reconciliation.py tests/unit_tests/flux/api/test_signals_inventory_contract.py tests/unit_tests/flux/api/test_payload_snapshots.py -q`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/strategies/makerv4/test_inventory_contract.py tests/unit_tests/flux/strategies/makerv4/test_strategy.py tests/unit_tests/flux/strategies/makerv4/test_instruments.py -q`

**Step 1: Write the failing tests**

Pin:

- managed-order collection and cancel-safety invariants
- inventory/exposure math invariants
- common market-identity normalization invariants used by the migrated helpers
- API inventory projection semantics
- representative golden fixtures for inventory and order-intent linkage payloads

**Step 2: Run tests to verify they fail**

Run the shared/Makerv3/API slice in `default-unit` and the Makerv4 slice in `ibkr-unit`.

**Step 3: Commit**

```bash
git add tests/unit_tests/flux/strategies/makerv3/test_managed_orders.py \
  tests/unit_tests/flux/strategies/makerv3/test_inventory.py \
  tests/unit_tests/flux/strategies/makerv4/test_inventory_contract.py \
  tests/unit_tests/flux/strategies/makerv4/test_strategy.py \
  tests/unit_tests/flux/strategies/makerv4/test_instruments.py \
  tests/unit_tests/flux/test_architecture_boundaries.py \
  tests/unit_tests/flux/common/test_market_identity.py \
  tests/unit_tests/flux/common/test_strategy_contracts.py \
  tests/unit_tests/flux/api/test_signals_inventory_contract.py \
  tests/unit_tests/flux/api/test_payload_snapshots.py
git commit -m "test: lock shared execution primitive contracts"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 2: Extract shared managed-order helpers and migrate consumers

**Files:**
- Create: `systems/flux/flux/strategies/shared/managed_orders.py`
- Create: `tests/unit_tests/flux/strategies/shared/test_managed_orders.py`
- Modify: `systems/flux/flux/strategies/makerv3/managed_orders.py`
- Modify: `systems/flux/flux/strategies/makerv4/strategy.py`
- Modify: `tests/unit_tests/flux/strategies/makerv3/test_managed_orders.py`

**Dependencies:** `Task 1: Lock shared managed-order and inventory contracts in tests`

**Write Scope:** `systems/flux/flux/strategies/shared/managed_orders.py`, `tests/unit_tests/flux/strategies/shared/test_managed_orders.py`, `systems/flux/flux/strategies/makerv3/managed_orders.py`, `systems/flux/flux/strategies/makerv4/strategy.py`, `tests/unit_tests/flux/strategies/makerv3/test_managed_orders.py`

**Verification Commands:**
- `python -c "import ibapi"`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/strategies/shared/test_managed_orders.py tests/unit_tests/flux/strategies/makerv3/test_managed_orders.py tests/unit_tests/flux/strategies/makerv4/test_strategy.py -q -k 'managed_order or cancel'`

**Step 1: Add direct shared tests**

Cover pure helper behavior directly under `tests/unit_tests/flux/strategies/shared`.

**Step 2: Move reusable helpers**

Extract collection, filtering, and cancel-safety primitives into `shared/managed_orders.py`.

**Step 3: Migrate Makerv3 and Makerv4**

Makerv3 and Makerv4 must both consume the shared helper in this PR.

**Step 4: Commit**

```bash
git add systems/flux/flux/strategies/shared/managed_orders.py \
  tests/unit_tests/flux/strategies/shared/test_managed_orders.py \
  systems/flux/flux/strategies/makerv3/managed_orders.py \
  systems/flux/flux/strategies/makerv4/strategy.py \
  tests/unit_tests/flux/strategies/makerv3/test_managed_orders.py
git commit -m "refactor: extract shared managed order helpers"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 3: Extract shared inventory adapters and migrate consumers

**Files:**
- Modify: `systems/flux/flux/common/strategy_contracts.py`
- Create: `systems/flux/flux/strategies/shared/inventory_math.py`
- Create: `tests/unit_tests/flux/strategies/shared/test_inventory_math.py`
- Modify: `systems/flux/flux/strategies/makerv3/inventory.py`
- Modify: `systems/flux/flux/strategies/makerv3/strategy.py`
- Modify: `systems/flux/flux/strategies/makerv4/strategy.py`
- Modify: `tests/unit_tests/flux/common/test_strategy_contracts.py`
- Modify: `tests/unit_tests/flux/strategies/makerv3/test_inventory.py`
- Modify: `tests/unit_tests/flux/strategies/makerv4/test_inventory_contract.py`
- Modify: `tests/unit_tests/flux/strategies/makerv4/test_instruments.py`
- Modify: `tests/unit_tests/flux/api/test_signals_inventory_contract.py`
- Modify: `tests/unit_tests/flux/api/test_payload_snapshots.py`
- Modify: `tests/unit_tests/flux/test_architecture_boundaries.py`

**Dependencies:** `Task 2: Extract shared managed-order helpers and migrate consumers`

**Write Scope:** `systems/flux/flux/common/strategy_contracts.py`, `systems/flux/flux/strategies/shared/inventory_math.py`, `tests/unit_tests/flux/strategies/shared/test_inventory_math.py`, `systems/flux/flux/strategies/makerv3/inventory.py`, `systems/flux/flux/strategies/makerv3/strategy.py`, `systems/flux/flux/strategies/makerv4/strategy.py`, `tests/unit_tests/flux/common/test_strategy_contracts.py`, `tests/unit_tests/flux/strategies/makerv3/test_inventory.py`, `tests/unit_tests/flux/strategies/makerv4/test_inventory_contract.py`, `tests/unit_tests/flux/strategies/makerv4/test_instruments.py`, `tests/unit_tests/flux/api/test_signals_inventory_contract.py`, `tests/unit_tests/flux/api/test_payload_snapshots.py`, `tests/unit_tests/flux/test_architecture_boundaries.py`

**Verification Commands:**
- `python -c "import ibapi"`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/test_architecture_boundaries.py tests/unit_tests/flux/common/test_market_identity.py tests/unit_tests/flux/common/test_strategy_contracts.py tests/unit_tests/flux/strategies/shared/test_inventory_math.py tests/unit_tests/flux/strategies/makerv3/test_inventory.py tests/unit_tests/flux/api/test_signals_inventory_contract.py tests/unit_tests/flux/api/test_payload_snapshots.py -q`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/strategies/makerv4/test_inventory_contract.py tests/unit_tests/flux/strategies/makerv4/test_strategy.py tests/unit_tests/flux/strategies/makerv4/test_instruments.py -q`

**Step 1: Add direct shared inventory-adapter tests**

Cover conversion and exposure math without strategy objects.

**Step 2: Move only the reusable helpers**

Do not move family-local skew or policy logic that is not clearly shared. The shared module should compose `flux.common` quantity/projection helpers rather than replacing them.

**Step 3: Remove duplicated market-type heuristics from the migrated path**

Route the migrated Makerv3 and Makerv4 execution helpers through the typed `flux.common.market_identity` surface rather than direct `-SPOT.` / `-PERP.` / `_SPOT` checks or a new hidden normalization layer.

**Step 4: Migrate Makerv3 and Makerv4**

Update both families and the API-facing tests to use the new shared helper ownership.

**Step 5: Commit**

```bash
git add systems/flux/flux/common/strategy_contracts.py \
  systems/flux/flux/strategies/shared/inventory_math.py \
  tests/unit_tests/flux/strategies/shared/test_inventory_math.py \
  systems/flux/flux/strategies/makerv3/inventory.py \
  systems/flux/flux/strategies/makerv3/strategy.py \
  systems/flux/flux/strategies/makerv4/strategy.py \
  tests/unit_tests/flux/common/test_strategy_contracts.py \
  tests/unit_tests/flux/strategies/makerv3/test_inventory.py \
  tests/unit_tests/flux/strategies/makerv4/test_inventory_contract.py \
  tests/unit_tests/flux/strategies/makerv4/test_instruments.py \
  tests/unit_tests/flux/api/test_signals_inventory_contract.py \
  tests/unit_tests/flux/api/test_payload_snapshots.py \
  tests/unit_tests/flux/test_architecture_boundaries.py
git commit -m "refactor: extract shared inventory math helpers"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 4: Verify import cleanup and record rollback note

**Files:**
- Modify: `systems/flux/docs/makerv3.md`
- Modify: `docs/plans/2026-03-31-strategy-platform-hardening-wave-pr4-shared-execution-primitives.md`

**Dependencies:** `Task 3: Extract shared inventory adapters and migrate consumers`

**Write Scope:** `systems/flux/docs/makerv3.md`, `docs/plans/2026-03-31-strategy-platform-hardening-wave-pr4-shared-execution-primitives.md`

**Verification Commands:**
- `python -c "import ibapi"`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/test_architecture_boundaries.py tests/unit_tests/flux/common/test_market_identity.py tests/unit_tests/flux/common/test_strategy_contracts.py tests/unit_tests/flux/strategies/shared/test_managed_orders.py tests/unit_tests/flux/strategies/shared/test_inventory_math.py tests/unit_tests/flux/strategies/makerv3/test_managed_orders.py tests/unit_tests/flux/strategies/makerv3/test_inventory.py tests/unit_tests/flux/strategies/makerv3/test_order_safety.py tests/unit_tests/flux/strategies/makerv3/test_reconciliation.py tests/unit_tests/flux/api/test_signals_inventory_contract.py tests/unit_tests/flux/api/test_payload_snapshots.py -q`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/strategies/makerv4/test_inventory_contract.py tests/unit_tests/flux/strategies/makerv4/test_strategy.py tests/unit_tests/flux/strategies/makerv4/test_instruments.py -q`
- `rg -n "from flux\\.strategies\\.makerv3 import inventory|from flux\\.strategies\\.makerv3 import managed_orders" systems/flux/flux/strategies/makerv4`

**Step 1: Run the combined verification bundle**

The direct shared tests, strategy tests, order-safety/reconciliation slices, and API inventory contract tests must pass together.

**Step 2: Verify Makerv4 import cleanup**

Makerv4 must no longer import the extracted helpers from Makerv3.

**Step 3: Record rollback note**

State explicitly that external payloads were preserved while internal helper ownership changed.

**Step 4: Commit**

```bash
git add systems/flux/docs/makerv3.md \
  docs/plans/2026-03-31-strategy-platform-hardening-wave-pr4-shared-execution-primitives.md
git commit -m "docs: record pr4 verification and rollback"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.



---

## Source: `docs/plans/2026-03-31-strategy-platform-hardening-wave-pr5-makerv3-quote-pipeline-split.md`

# Strategy Platform Hardening Wave PR5 MakerV3 Quote Pipeline Split Implementation Plan

> **For the execution agent:** REQUIRED SUB-SKILL: Before implementing this plan, choose exactly one execution mode and use the matching skill: `superpowers:subagent-driven-development` for same-session execution or `superpowers:executing-plans` for a separate-session handoff.
> **Progress:** The Progress Tracker in this document is the execution source of truth and must be updated on every state change.

**Goal:** Split the Makerv3 quote-refresh hot path into explicit collaborators without changing quoting behavior, operator payloads, or safety gates.

**Architecture:** Break `quote_engine.refresh_quotes` into family-local collaborator modules for preflight/gating, target assembly, action execution, and telemetry recording. Preserve all existing risk, stale-data, pending-cancel, and observability behavior while making the hot path auditable and easier to test directly.

**Tech Stack:** Python strategy/runtime code, Makerv3 quote engine, pytest.

**Context Docs:**
- Design: `docs/plans/2026-03-31-strategy-platform-hardening-wave-design.md`
- PRD: `docs/prd/2026-03-31-strategy-platform-hardening-wave.md`
- Relevant specs/runbooks: `systems/flux/docs/makerv3.md`, `docs/plans/2026-03-26-shared-deque-quote-stack-design.md`

**Decision Summary:**
- this is a structural PR only
- no external contract changes are allowed
- new collaborator modules stay family-local because this split is not a new shared boundary

## Release Gates

| Environment | Required for merge? | Requirement |
| --- | --- | --- |
| `default-unit` | yes | quote-engine, order-intent, and observability proof must pass |
| `pilot` | yes | deploy one pinned tokenmm pilot release from the PR head and validate the smoke bundle below |

## Affected Pilot Surfaces

- tokenmm quote lifecycle and quote refresh behavior
- order-intent exports and quote telemetry
- Makerv3 operator state and alert surfaces that reflect quote progress

## Pilot Deploy Units And Promotion Order

Deploy units:

- `flux.runners.tokenmm.run_node`
- `flux.api.app` if operator payloads or order-intent surfaces are served separately in pilot

Promotion order:

1. build one pinned pilot release from the exact PR head
2. deploy `flux.runners.tokenmm.run_node` and `flux.api.app` from that same release when both are in scope
3. validate quote lifecycle, order-intent, and telemetry smoke checks
4. promote the same units together only after the smoke bundle is clean

## Pilot Smoke Bundle

1. Deploy the tokenmm pilot release from the exact PR head.
2. Confirm quote refresh continues to place, amend, and cancel quotes in the same steady-state scenarios.
3. Confirm quote-cycle, order-intent, and observability payloads remain unchanged.
4. Confirm no new family-shared dependency is introduced by the split.

## PR-Local Rollback

- revert the whole PR
- redeploy the previous pinned pilot release
- do not mix old quote-engine code with partially extracted collaborator modules

## Progress Tracker

**Source of truth:** Update this table whenever task state changes. Do not rely on memory, chat history, or TodoWrite alone.

| Task | Status | Owner | Depends On | Write Scope | Lane Branch | Worktree Path | Commit / Diff | Verification | Notes / Last Update |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Overall | not_started | unassigned | none | `systems/flux/flux/strategies/makerv3`, `tests/unit_tests/flux/strategies/makerv3`, `systems/flux/docs` | `wave/pr5-quote-pipeline-split` | `.worktrees/strategy-platform-pr5` | none | not_run | Plan created |
| Task 1: Lock quote-pipeline behavior with focused regression tests | not_started | unassigned | none | `tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py`, `tests/unit_tests/flux/strategies/makerv3/test_order_intent_exports.py`, `tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py` | `wave/pr5-quote-pipeline-split` | `.worktrees/strategy-platform-pr5` | none | not_run | Plan created |
| Task 2: Extract preflight and target-building collaborators | not_started | unassigned | Task 1: Lock quote-pipeline behavior with focused regression tests | `systems/flux/flux/strategies/makerv3/quote_preflight.py`, `systems/flux/flux/strategies/makerv3/quote_targets.py`, `systems/flux/flux/strategies/makerv3/quote_engine.py`, `tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py` | `wave/pr5-quote-pipeline-split` | `.worktrees/strategy-platform-pr5` | none | not_run | Plan created |
| Task 3: Extract action-execution and telemetry collaborators | not_started | unassigned | Task 2: Extract preflight and target-building collaborators | `systems/flux/flux/strategies/makerv3/quote_actions.py`, `systems/flux/flux/strategies/makerv3/quote_telemetry.py`, `systems/flux/flux/strategies/makerv3/quote_engine.py`, `tests/unit_tests/flux/strategies/makerv3/test_order_intent_exports.py`, `tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py` | `wave/pr5-quote-pipeline-split` | `.worktrees/strategy-platform-pr5` | none | not_run | Plan created |
| Task 4: Verify behavior preservation and record rollback note | not_started | unassigned | Task 3: Extract action-execution and telemetry collaborators | `systems/flux/docs/makerv3.md`, `docs/plans/2026-03-31-strategy-platform-hardening-wave-pr5-makerv3-quote-pipeline-split.md` | `wave/pr5-quote-pipeline-split` | `.worktrees/strategy-platform-pr5` | none | not_run | Plan created |

---

### Task 1: Lock quote-pipeline behavior with focused regression tests

**Files:**
- Modify: `tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py`
- Modify: `tests/unit_tests/flux/strategies/makerv3/test_order_intent_exports.py`
- Modify: `tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py`

**Dependencies:** `none`

**Write Scope:** `tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py`, `tests/unit_tests/flux/strategies/makerv3/test_order_intent_exports.py`, `tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py`

**Verification Commands:**
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py tests/unit_tests/flux/strategies/makerv3/test_order_intent_exports.py tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py -q`

**Step 1: Write the failing tests**

Lock:

- stale-data blocks
- pending-cancel blocking
- action ordering
- quote-cycle and order-intent payload content

**Step 2: Run tests to verify they fail**

Use the focused command above.

**Step 3: Commit**

```bash
git add tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py \
  tests/unit_tests/flux/strategies/makerv3/test_order_intent_exports.py \
  tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py
git commit -m "test: lock makerv3 quote pipeline behavior"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 2: Extract preflight and target-building collaborators

**Files:**
- Create: `systems/flux/flux/strategies/makerv3/quote_preflight.py`
- Create: `systems/flux/flux/strategies/makerv3/quote_targets.py`
- Modify: `systems/flux/flux/strategies/makerv3/quote_engine.py`
- Modify: `tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py`

**Dependencies:** `Task 1: Lock quote-pipeline behavior with focused regression tests`

**Write Scope:** `systems/flux/flux/strategies/makerv3/quote_preflight.py`, `systems/flux/flux/strategies/makerv3/quote_targets.py`, `systems/flux/flux/strategies/makerv3/quote_engine.py`, `tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py`

**Verification Commands:**
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py -q`

**Step 1: Extract the preflight gate logic**

Move stale-data, startup cleanup, and pending-cancel admission logic into `quote_preflight.py`.

**Step 2: Extract target assembly**

Move desired target and side-planning preparation into `quote_targets.py`.

**Step 3: Re-run focused tests**

Confirm preflight and target behavior did not change.

**Step 4: Commit**

```bash
git add systems/flux/flux/strategies/makerv3/quote_preflight.py \
  systems/flux/flux/strategies/makerv3/quote_targets.py \
  systems/flux/flux/strategies/makerv3/quote_engine.py \
  tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py
git commit -m "refactor: extract makerv3 quote preflight and targets"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 3: Extract action-execution and telemetry collaborators

**Files:**
- Create: `systems/flux/flux/strategies/makerv3/quote_actions.py`
- Create: `systems/flux/flux/strategies/makerv3/quote_telemetry.py`
- Modify: `systems/flux/flux/strategies/makerv3/quote_engine.py`
- Modify: `tests/unit_tests/flux/strategies/makerv3/test_order_intent_exports.py`
- Modify: `tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py`

**Dependencies:** `Task 2: Extract preflight and target-building collaborators`

**Write Scope:** `systems/flux/flux/strategies/makerv3/quote_actions.py`, `systems/flux/flux/strategies/makerv3/quote_telemetry.py`, `systems/flux/flux/strategies/makerv3/quote_engine.py`, `tests/unit_tests/flux/strategies/makerv3/test_order_intent_exports.py`, `tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py`

**Verification Commands:**
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py tests/unit_tests/flux/strategies/makerv3/test_order_intent_exports.py tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py -q`

**Step 1: Extract order/cancel execution sequencing**

Move action execution logic to `quote_actions.py`.

**Step 2: Extract telemetry assembly**

Move quote-cycle diagnostics and related envelope helpers to `quote_telemetry.py`.

**Step 3: Re-run the combined bundle**

All focused quote-engine, order-intent, and observability tests must pass together.

**Step 4: Commit**

```bash
git add systems/flux/flux/strategies/makerv3/quote_actions.py \
  systems/flux/flux/strategies/makerv3/quote_telemetry.py \
  systems/flux/flux/strategies/makerv3/quote_engine.py \
  tests/unit_tests/flux/strategies/makerv3/test_order_intent_exports.py \
  tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py
git commit -m "refactor: extract makerv3 quote actions and telemetry"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 4: Verify behavior preservation and record rollback note

**Files:**
- Modify: `systems/flux/docs/makerv3.md`
- Modify: `docs/plans/2026-03-31-strategy-platform-hardening-wave-pr5-makerv3-quote-pipeline-split.md`

**Dependencies:** `Task 3: Extract action-execution and telemetry collaborators`

**Write Scope:** `systems/flux/docs/makerv3.md`, `docs/plans/2026-03-31-strategy-platform-hardening-wave-pr5-makerv3-quote-pipeline-split.md`

**Verification Commands:**
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py tests/unit_tests/flux/strategies/makerv3/test_order_intent_exports.py tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py -q`
- `git diff --check`

**Step 1: Run the focused bundle**

This PR is complete only when the entire focused quote-pipeline bundle is green.

**Step 2: Update docs and rollback note**

Document the new module layout and explicitly state that this was a structural split with no contract changes.

**Step 3: Commit**

```bash
git add systems/flux/docs/makerv3.md \
  docs/plans/2026-03-31-strategy-platform-hardening-wave-pr5-makerv3-quote-pipeline-split.md
git commit -m "docs: record pr5 verification and rollback"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.



---

## Source: `docs/plans/2026-03-31-strategy-platform-hardening-wave-pr6-makerv3-strategy-decomposition.md`

# Strategy Platform Hardening Wave PR6 MakerV3 Strategy Decomposition Implementation Plan

> **For the execution agent:** REQUIRED SUB-SKILL: Before implementing this plan, choose exactly one execution mode and use the matching skill: `superpowers:subagent-driven-development` for same-session execution or `superpowers:executing-plans` for a separate-session handoff.
> **Progress:** The Progress Tracker in this document is the execution source of truth and must be updated on every state change.

**Goal:** Shrink `MakerV3Strategy` into a smaller orchestration surface by extracting lifecycle, order-state bookkeeping, inventory-state assembly, and state-export orchestration helpers without changing behavior.

**Architecture:** Keep Makerv3 family-local behavior inside Makerv3, but split the current god object into collaborator modules that own coherent domains. The strategy class should become orchestration glue, not the place where every helper method lives.

**Tech Stack:** Python strategy/runtime code, Makerv3 lifecycle and state export tests, pytest.

**Context Docs:**
- Design: `docs/plans/2026-03-31-strategy-platform-hardening-wave-design.md`
- PRD: `docs/prd/2026-03-31-strategy-platform-hardening-wave.md`
- Relevant specs/runbooks: `systems/flux/docs/makerv3.md`

**Decision Summary:**
- this is family-local decomposition, not shared extraction
- collaborator modules must be named by responsibility, not by “misc” buckets
- public strategy behavior and exported payloads remain frozen

## Release Gates

| Environment | Required for merge? | Requirement |
| --- | --- | --- |
| `default-unit` | yes | lifecycle, reconciliation, runtime-param, and observability proof must pass |
| `pilot` | yes | deploy one pinned tokenmm pilot release from the PR head and validate the smoke bundle below |

## Affected Pilot Surfaces

- tokenmm lifecycle startup and shutdown
- runtime-param manager wiring
- state export and reconciliation behavior

## Pilot Deploy Units And Promotion Order

Deploy units:

- `flux.runners.tokenmm.run_node`
- `flux.api.app` if state-export payloads are served separately in pilot

Promotion order:

1. build one pinned pilot release from the exact PR head
2. deploy `flux.runners.tokenmm.run_node` and `flux.api.app` from that same release when both are in scope
3. validate lifecycle, runtime-param, reconciliation, and state-export smoke checks
4. promote the same units together only after the smoke bundle is clean

## Pilot Smoke Bundle

1. Deploy the tokenmm pilot release from the exact PR head.
2. Confirm startup, steady-state running, and shutdown remain clean.
3. Confirm runtime-param management and state export payloads remain unchanged.
4. Confirm reconciliation and pending-cancel bookkeeping behave the same during steady-state operation.

## PR-Local Rollback

- revert the whole PR
- redeploy the previous pinned pilot release
- do not mix the old strategy class with only some extracted collaborator modules

## Progress Tracker

**Source of truth:** Update this table whenever task state changes. Do not rely on memory, chat history, or TodoWrite alone.

| Task | Status | Owner | Depends On | Write Scope | Lane Branch | Worktree Path | Commit / Diff | Verification | Notes / Last Update |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Overall | not_started | unassigned | none | `systems/flux/flux/strategies/makerv3`, `tests/unit_tests/flux/strategies/makerv3`, `systems/flux/docs` | `wave/pr6-strategy-decomposition` | `.worktrees/strategy-platform-pr6` | none | not_run | Plan created |
| Task 1: Lock lifecycle and state-export behavior in tests | not_started | unassigned | none | `tests/unit_tests/flux/strategies/makerv3/test_strategy_lifecycle.py`, `tests/unit_tests/flux/strategies/makerv3/test_reconciliation.py`, `tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py`, `tests/unit_tests/flux/strategies/makerv3/test_runtime_params.py` | `wave/pr6-strategy-decomposition` | `.worktrees/strategy-platform-pr6` | none | not_run | Plan created |
| Task 2: Extract lifecycle and order-state collaborators | not_started | unassigned | Task 1: Lock lifecycle and state-export behavior in tests | `systems/flux/flux/strategies/makerv3/lifecycle.py`, `systems/flux/flux/strategies/makerv3/order_state.py`, `systems/flux/flux/strategies/makerv3/strategy.py`, `tests/unit_tests/flux/strategies/makerv3/test_strategy_lifecycle.py`, `tests/unit_tests/flux/strategies/makerv3/test_reconciliation.py` | `wave/pr6-strategy-decomposition` | `.worktrees/strategy-platform-pr6` | none | not_run | Plan created |
| Task 3: Extract inventory-state and state-export collaborators | not_started | unassigned | Task 2: Extract lifecycle and order-state collaborators | `systems/flux/flux/strategies/makerv3/inventory_state.py`, `systems/flux/flux/strategies/makerv3/state_exports.py`, `systems/flux/flux/strategies/makerv3/strategy.py`, `tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py`, `tests/unit_tests/flux/strategies/makerv3/test_runtime_params.py` | `wave/pr6-strategy-decomposition` | `.worktrees/strategy-platform-pr6` | none | not_run | Plan created |
| Task 4: Verify shrinkdown and record rollback note | not_started | unassigned | Task 3: Extract inventory-state and state-export collaborators | `systems/flux/docs/makerv3.md`, `docs/plans/2026-03-31-strategy-platform-hardening-wave-pr6-makerv3-strategy-decomposition.md` | `wave/pr6-strategy-decomposition` | `.worktrees/strategy-platform-pr6` | none | not_run | Plan created |

---

### Task 1: Lock lifecycle and state-export behavior in tests

**Files:**
- Modify: `tests/unit_tests/flux/strategies/makerv3/test_strategy_lifecycle.py`
- Modify: `tests/unit_tests/flux/strategies/makerv3/test_reconciliation.py`
- Modify: `tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py`
- Modify: `tests/unit_tests/flux/strategies/makerv3/test_runtime_params.py`

**Dependencies:** `none`

**Write Scope:** `tests/unit_tests/flux/strategies/makerv3/test_strategy_lifecycle.py`, `tests/unit_tests/flux/strategies/makerv3/test_reconciliation.py`, `tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py`, `tests/unit_tests/flux/strategies/makerv3/test_runtime_params.py`

**Verification Commands:**
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/strategies/makerv3/test_strategy_lifecycle.py tests/unit_tests/flux/strategies/makerv3/test_reconciliation.py tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py tests/unit_tests/flux/strategies/makerv3/test_runtime_params.py -q`

**Step 1: Write the failing tests**

Lock:

- startup/shutdown transitions
- order-event bookkeeping and reconciliation
- state export behavior
- runtime-param manager wiring

**Step 2: Run tests to verify they fail**

Use the focused command above.

**Step 3: Commit**

```bash
git add tests/unit_tests/flux/strategies/makerv3/test_strategy_lifecycle.py \
  tests/unit_tests/flux/strategies/makerv3/test_reconciliation.py \
  tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py \
  tests/unit_tests/flux/strategies/makerv3/test_runtime_params.py
git commit -m "test: lock makerv3 strategy decomposition behavior"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 2: Extract lifecycle and order-state collaborators

**Files:**
- Create: `systems/flux/flux/strategies/makerv3/lifecycle.py`
- Create: `systems/flux/flux/strategies/makerv3/order_state.py`
- Modify: `systems/flux/flux/strategies/makerv3/strategy.py`
- Modify: `tests/unit_tests/flux/strategies/makerv3/test_strategy_lifecycle.py`
- Modify: `tests/unit_tests/flux/strategies/makerv3/test_reconciliation.py`

**Dependencies:** `Task 1: Lock lifecycle and state-export behavior in tests`

**Write Scope:** `systems/flux/flux/strategies/makerv3/lifecycle.py`, `systems/flux/flux/strategies/makerv3/order_state.py`, `systems/flux/flux/strategies/makerv3/strategy.py`, `tests/unit_tests/flux/strategies/makerv3/test_strategy_lifecycle.py`, `tests/unit_tests/flux/strategies/makerv3/test_reconciliation.py`

**Verification Commands:**
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/strategies/makerv3/test_strategy_lifecycle.py tests/unit_tests/flux/strategies/makerv3/test_reconciliation.py -q`

**Step 1: Extract startup/shutdown wiring**

Move lifecycle-specific logic into `lifecycle.py`.

**Step 2: Extract pending-cancel and order-event bookkeeping**

Move order-state tracking into `order_state.py`.

**Step 3: Re-run focused tests**

Confirm lifecycle and reconciliation behavior is preserved.

**Step 4: Commit**

```bash
git add systems/flux/flux/strategies/makerv3/lifecycle.py \
  systems/flux/flux/strategies/makerv3/order_state.py \
  systems/flux/flux/strategies/makerv3/strategy.py \
  tests/unit_tests/flux/strategies/makerv3/test_strategy_lifecycle.py \
  tests/unit_tests/flux/strategies/makerv3/test_reconciliation.py
git commit -m "refactor: extract makerv3 lifecycle and order state"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 3: Extract inventory-state and state-export collaborators

**Files:**
- Create: `systems/flux/flux/strategies/makerv3/inventory_state.py`
- Create: `systems/flux/flux/strategies/makerv3/state_exports.py`
- Modify: `systems/flux/flux/strategies/makerv3/strategy.py`
- Modify: `tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py`
- Modify: `tests/unit_tests/flux/strategies/makerv3/test_runtime_params.py`

**Dependencies:** `Task 2: Extract lifecycle and order-state collaborators`

**Write Scope:** `systems/flux/flux/strategies/makerv3/inventory_state.py`, `systems/flux/flux/strategies/makerv3/state_exports.py`, `systems/flux/flux/strategies/makerv3/strategy.py`, `tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py`, `tests/unit_tests/flux/strategies/makerv3/test_runtime_params.py`

**Verification Commands:**
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py tests/unit_tests/flux/strategies/makerv3/test_runtime_params.py -q`

**Step 1: Extract inventory-state assembly**

Move strategy-local inventory-state gathering into `inventory_state.py`.

**Step 2: Extract state-export orchestration**

Move state payload assembly orchestration into `state_exports.py`.

**Step 3: Re-run focused tests**

Confirm exported payloads and runtime-param behavior stay unchanged.

**Step 4: Commit**

```bash
git add systems/flux/flux/strategies/makerv3/inventory_state.py \
  systems/flux/flux/strategies/makerv3/state_exports.py \
  systems/flux/flux/strategies/makerv3/strategy.py \
  tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py \
  tests/unit_tests/flux/strategies/makerv3/test_runtime_params.py
git commit -m "refactor: extract makerv3 inventory and state export collaborators"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 4: Verify shrinkdown and record rollback note

**Files:**
- Modify: `systems/flux/docs/makerv3.md`
- Modify: `docs/plans/2026-03-31-strategy-platform-hardening-wave-pr6-makerv3-strategy-decomposition.md`

**Dependencies:** `Task 3: Extract inventory-state and state-export collaborators`

**Write Scope:** `systems/flux/docs/makerv3.md`, `docs/plans/2026-03-31-strategy-platform-hardening-wave-pr6-makerv3-strategy-decomposition.md`

**Verification Commands:**
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/strategies/makerv3/test_strategy_lifecycle.py tests/unit_tests/flux/strategies/makerv3/test_reconciliation.py tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py tests/unit_tests/flux/strategies/makerv3/test_runtime_params.py -q`
- `git diff --check`

**Step 1: Run the combined verification bundle**

The lifecycle, reconciliation, observability, and runtime-param slices must pass together.

**Step 2: Update docs and rollback note**

Document the new Makerv3 internal collaborator layout and that public behavior remained frozen.

**Step 3: Commit**

```bash
git add systems/flux/docs/makerv3.md \
  docs/plans/2026-03-31-strategy-platform-hardening-wave-pr6-makerv3-strategy-decomposition.md
git commit -m "docs: record pr6 verification and rollback"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.



---

## Source: `docs/plans/2026-03-31-strategy-platform-hardening-wave-pr7-docstrings-runbooks-and-cleanup.md`

# Strategy Platform Hardening Wave PR7 Docstrings Runbooks And Cleanup Implementation Plan

> **For the execution agent:** REQUIRED SUB-SKILL: Before implementing this plan, choose exactly one execution mode and use the matching skill: `superpowers:subagent-driven-development` for same-session execution or `superpowers:executing-plans` for a separate-session handoff.
> **Progress:** The Progress Tracker in this document is the execution source of truth and must be updated on every state change.

**Goal:** Finish the wave with residual documentation, docstring, runbook, and cleanup work after the code boundaries are stable, without deferring earlier PR contract coverage into this final PR.

**Architecture:** Use this PR only for residual cleanup that becomes clearer after the wave lands: platform docs, runbooks, docstrings on newly public shared/common modules, and small invariant tests for PR5/PR6-created modules. This PR is not allowed to backfill contract tests or rollback notes that earlier PRs should have shipped.

**Tech Stack:** Markdown docs, Python docstrings, docs tests, lint, pytest.

**Context Docs:**
- Design: `docs/plans/2026-03-31-strategy-platform-hardening-wave-design.md`
- PRD: `docs/prd/2026-03-31-strategy-platform-hardening-wave.md`
- Relevant specs/runbooks: `systems/flux/docs/makerv3.md`, `docs/runbooks/tokenmm-makerv3-bounded-convergence-rollout.md`

**Decision Summary:**
- earlier PRs own their own contract tests and rollback notes
- this PR is for residual narrative/documentation closeout only
- any new invariants added here are for modules created late in the wave, not for deferred shared extraction coverage

## Release Gates

| Environment | Required for merge? | Requirement |
| --- | --- | --- |
| `default-unit` | yes | docs tests, residual invariants, and docstring lint must pass |

This PR does not require a pilot release gate unless it unexpectedly stops being docs-and-invariants-only. If code behavior or operator contracts change, the work belongs in an earlier PR instead.

## Progress Tracker

**Source of truth:** Update this table whenever task state changes. Do not rely on memory, chat history, or TodoWrite alone.

| Task | Status | Owner | Depends On | Write Scope | Lane Branch | Worktree Path | Commit / Diff | Verification | Notes / Last Update |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Overall | not_started | unassigned | none | `systems/flux/docs`, `docs/runbooks`, `docs/plans`, `systems/flux/flux/common`, `systems/flux/flux/strategies/shared`, `systems/flux/flux/strategies/makerv3`, `tests/unit_tests/docs`, `tests/unit_tests/flux/strategies/shared`, `tests/unit_tests/flux/strategies/makerv3` | `wave/pr7-docs-cleanup` | `.worktrees/strategy-platform-pr7` | none | not_run | Plan created |
| Task 1: Audit residual docstring and docs gaps from the wave | not_started | unassigned | none | `systems/flux/docs`, `docs/runbooks`, `docs/plans`, `systems/flux/flux/common`, `systems/flux/flux/strategies/shared`, `systems/flux/flux/strategies/makerv3` | `wave/pr7-docs-cleanup` | `.worktrees/strategy-platform-pr7` | none | not_run | Plan created |
| Task 2: Update platform docs and operator runbooks | not_started | unassigned | Task 1: Audit residual docstring and docs gaps from the wave | `systems/flux/docs/makerv3.md`, `systems/flux/docs/strategy_platform.md`, `docs/runbooks/tokenmm-makerv3-bounded-convergence-rollout.md`, `docs/plans/2026-03-31-strategy-platform-hardening-wave-review-packet.md` | `wave/pr7-docs-cleanup` | `.worktrees/strategy-platform-pr7` | none | not_run | Plan created |
| Task 3: Add residual public docstrings and direct invariants for late-created modules | not_started | unassigned | Task 2: Update platform docs and operator runbooks | `systems/flux/flux/common/*.py`, `systems/flux/flux/strategies/shared/*.py`, `systems/flux/flux/strategies/makerv3/*.py`, `tests/unit_tests/flux/strategies/shared`, `tests/unit_tests/flux/strategies/makerv3` | `wave/pr7-docs-cleanup` | `.worktrees/strategy-platform-pr7` | none | not_run | Plan created |
| Task 4: Run docs and lint verification, then record rollback note | not_started | unassigned | Task 3: Add residual public docstrings and direct invariants for late-created modules | `docs/plans/2026-03-31-strategy-platform-hardening-wave-pr7-docstrings-runbooks-and-cleanup.md` | `wave/pr7-docs-cleanup` | `.worktrees/strategy-platform-pr7` | none | not_run | Plan created |

---

### Task 1: Audit residual docstring and docs gaps from the wave

**Files:**
- Modify: `docs/plans/2026-03-31-strategy-platform-hardening-wave-pr7-docstrings-runbooks-and-cleanup.md`

**Dependencies:** `none`

**Write Scope:** `docs/plans/2026-03-31-strategy-platform-hardening-wave-pr7-docstrings-runbooks-and-cleanup.md`

**Verification Commands:**
- `rg -n "^def |^class " systems/flux/flux/common systems/flux/flux/strategies/shared systems/flux/flux/strategies/makerv3`
- `rg -n "TODO|TBD|legacy contract|update docs" systems/flux/docs docs/runbooks docs/plans`

**Step 1: Enumerate the residual gaps**

List modules and doc surfaces that still need cleanup after PR0-PR6.

**Step 2: Confirm the gaps are truly residual**

Do not use this PR to smuggle in missed contract work from earlier PRs.

**Step 3: Commit**

```bash
git add docs/plans/2026-03-31-strategy-platform-hardening-wave-pr7-docstrings-runbooks-and-cleanup.md
git commit -m "docs: audit residual wave cleanup scope"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 2: Update platform docs and operator runbooks

**Files:**
- Modify: `systems/flux/docs/makerv3.md`
- Create: `systems/flux/docs/strategy_platform.md`
- Modify: `docs/runbooks/tokenmm-makerv3-bounded-convergence-rollout.md`
- Modify: `docs/plans/2026-03-31-strategy-platform-hardening-wave-review-packet.md`

**Dependencies:** `Task 1: Audit residual docstring and docs gaps from the wave`

**Write Scope:** `systems/flux/docs/makerv3.md`, `systems/flux/docs/strategy_platform.md`, `docs/runbooks/tokenmm-makerv3-bounded-convergence-rollout.md`, `docs/plans/2026-03-31-strategy-platform-hardening-wave-review-packet.md`

**Verification Commands:**
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/docs -q`

**Step 1: Update the canonical docs**

Describe the final platform layering and the new Makerv3 internal module layout.

**Step 2: Update runbooks**

Make operator guidance match the final ownership and observability model.

**Step 3: Commit**

```bash
git add systems/flux/docs/makerv3.md \
  systems/flux/docs/strategy_platform.md \
  docs/runbooks/tokenmm-makerv3-bounded-convergence-rollout.md \
  docs/plans/2026-03-31-strategy-platform-hardening-wave-review-packet.md
git commit -m "docs: update platform and operator documentation"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 3: Add residual public docstrings and direct invariants for late-created modules

**Files:**
- Modify: `systems/flux/flux/common/*.py`
- Modify: `systems/flux/flux/strategies/shared/*.py`
- Modify: `systems/flux/flux/strategies/makerv3/*.py`
- Modify: `tests/unit_tests/flux/strategies/shared/*`
- Modify: `tests/unit_tests/flux/strategies/makerv3/*`

**Dependencies:** `Task 2: Update platform docs and operator runbooks`

**Write Scope:** `systems/flux/flux/common/*.py`, `systems/flux/flux/strategies/shared/*.py`, `systems/flux/flux/strategies/makerv3/*.py`, `tests/unit_tests/flux/strategies/shared/*`, `tests/unit_tests/flux/strategies/makerv3/*`

**Verification Commands:**
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/strategies/shared tests/unit_tests/flux/strategies/makerv3 -q`
- `ruff check --select D systems/flux/flux/common systems/flux/flux/strategies/shared systems/flux/flux/strategies/makerv3`

**Step 1: Add missing public docstrings**

Focus on public modules and contracts created late in the wave.

**Step 2: Add residual direct invariants**

Only add direct tests for late-created public modules that do not already have strong focused coverage.

**Step 3: Commit**

```bash
git add systems/flux/flux/common \
  systems/flux/flux/strategies/shared \
  systems/flux/flux/strategies/makerv3 \
  tests/unit_tests/flux/strategies/shared \
  tests/unit_tests/flux/strategies/makerv3
git commit -m "docs: complete residual docstrings and invariants"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 4: Run docs and lint verification, then record rollback note

**Files:**
- Modify: `docs/plans/2026-03-31-strategy-platform-hardening-wave-pr7-docstrings-runbooks-and-cleanup.md`

**Dependencies:** `Task 3: Add residual public docstrings and direct invariants for late-created modules`

**Write Scope:** `docs/plans/2026-03-31-strategy-platform-hardening-wave-pr7-docstrings-runbooks-and-cleanup.md`

**Verification Commands:**
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/docs tests/unit_tests/flux/strategies/shared tests/unit_tests/flux/strategies/makerv3 -q`
- `ruff check --select D systems/flux/flux/common systems/flux/flux/strategies/shared systems/flux/flux/strategies/makerv3`
- `git diff --check`

**Step 1: Run the closeout bundle**

Docs tests, residual invariants, and docstring lint must all pass.

**Step 2: Record rollback note**

State explicitly that this PR is documentation and residual invariants only, so whole-PR revert is straightforward.

**Step 3: Commit**

```bash
git add docs/plans/2026-03-31-strategy-platform-hardening-wave-pr7-docstrings-runbooks-and-cleanup.md
git commit -m "docs: record pr7 verification and rollback"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

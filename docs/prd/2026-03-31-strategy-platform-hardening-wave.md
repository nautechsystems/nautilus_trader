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

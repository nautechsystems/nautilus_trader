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

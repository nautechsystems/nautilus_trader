# TokenMM Startup Auto-Repair Design

**Goal:** Make startup reconciliation auto-repair proven stale or double-applied local execution state without weakening the current fail-closed guarantees for ambiguous cases.

## Context

On March 26, 2026, both `plumeusdt_binance_spot_makerv3` and `plumeusdt_binance_perp_makerv3` failed startup reconciliation with `generate_missing_orders = false`.

The current startup path in [execution_engine.py](/home/ubuntu/nautilus_trader/nautilus_trader/live/execution_engine.py):
- switches to `open_only=True` order queries when the startup snapshot already has cached positions
- skips partial-window fill adjustment when cached positions already exist
- only auto-cleans stale startup netting positions when venue qty is flat and no startup open orders still look open in cache
- fails closed on any remaining net qty mismatch

Those protections are directionally correct, but they leave a gap when the local cache is stale or when startup history is only partially represented.

## Problem

The March 26 incident exposed two distinct startup failure modes:

1. A stale cached position can survive restart when one cached startup open order still looks open locally, even though venue truth is already flat.
2. A startup fill can be replayed or misattributed on top of already-cached position state, causing local net qty to move away from venue truth during restart.

The local cache is useful as a recovery aid, but it is not reliable enough to be treated as primary truth during startup mismatch handling.

## Evidence

### Binance Spot

- Cached position at shutdown: `-4000.8`
- Cached open order at shutdown: one residual buy order
- Startup venue position: `0`
- Startup venue fills: `3`
- Startup path: `open_only=True`, fill adjustment skipped, stale-position cleanup blocked, hard fail

This was a stale-cache incident that should have auto-repaired.

### Binance Perp

- Cached residual positions at shutdown: `28000` owned plus `2000` external
- Effective cached qty before restart: `30000`, matching venue
- Startup venue position: `30000`
- Startup venue fills: `2`
- Startup result: local qty became `31000`, then hard fail

This was not just stale residual state. Startup replayed or misattributed one extra `1000` during recovery.

## Decision

Keep the current fail-closed behavior as the fallback, but insert an evidence-gated startup auto-repair layer for netting instruments.

Startup should auto-repair only when venue truth plus startup evidence proves one bounded explanation for the mismatch. If the explanation is ambiguous, startup must still fail closed.

## Evidence Model

Startup auto-repair should classify each instrument from the following evidence:

- `StartupInstrumentCacheSnapshot`
- bulk open-order reports
- targeted open-order reports
- startup fill reports
- venue position reports
- cached orders, cached fills, and cached positions

The key rule is:

> Venue position truth wins, but only after startup proves whether recent fills or cached lineage are already represented locally.

## Startup Repair Policy

For startup netting reconciliation, the engine should evaluate repair outcomes in this order:

### 1. No Repair Needed

If venue qty already matches effective cached qty after excluding proven stale artifacts, startup continues with no repair.

### 2. Stale Cached Open Order And Stale Cached Position

If venue qty is flat and startup proves that the cached startup open orders are missing at venue:
- mark those cached open orders missing at venue
- recompute current startup open orders after that resolution
- close stale cached startup positions automatically

This fixes the Binance spot failure class.

### 3. Missing Orphan Lineage

If venue qty differs from cached qty only by a bounded same-strategy orphan fragment:
- restore the missing lineage from cached closed orders
- only accept exact qty matches
- only combine fragments within a bounded subset search
- never combine across strategies
- fail if multiple equally valid subsets exist

This restores the useful part of the March 25 orphan recovery without reopening unbounded subset guessing.

### 4. Startup Fill Not Yet Represented Locally

If startup fills explain the remaining venue delta, apply them only when they are not already represented in cache.

This must use the existing duplicate-fill safeguards:
- `trade_id` dedupe
- existing fill lookup
- inferred-fill timestamp guard
- fill application audit trail

This fixes the class where startup currently ignores needed closed fills because `snapshot.has_open_positions` forced a blanket skip.

### 5. Ambiguous Or Conflicting Evidence

If more than one repair path could explain the mismatch, or if venue history is incomplete enough that the engine cannot prove a single explanation, startup still fails closed.

## Engine Changes

The minimal engine change is inside [execution_engine.py](/home/ubuntu/nautilus_trader/nautilus_trader/live/execution_engine.py).

### Replace Blanket Open-Position Fill Skip

Today startup skips partial-window fill adjustment whenever the snapshot already has open positions. That is too coarse.

Instead:
- keep `open_only=True` order queries when appropriate
- allow bounded startup fill reasoning even when cached positions exist
- only skip startup fills already represented by cached `trade_id`s or inferred reconciliation fills

### Recompute Startup Open-Order Guard After Missing-At-Venue Resolution

The stale-position cleanup guard currently checks `current_startup_open_orders` before position cleanup, but it can still be blocked by cached open orders that startup has already proven missing at venue.

The guard should operate on post-resolution order state, not pre-resolution cache state.

### Reintroduce Bounded Grouped Orphan Restore

The current "single exact fragment only" orphan restore is too narrow for grouped same-strategy lineage. The previous exact-subset search was closer to the needed behavior, but it must stay bounded and strategy-scoped.

### Add Explicit Startup Mismatch Classification

Classify startup mismatch outcomes explicitly:
- `stale_cached_positions`
- `stale_cached_open_orders`
- `missing_orphan_lineage`
- `startup_fill_already_represented`
- `startup_fill_missing_locally`
- `ambiguous_startup_mismatch`

That classification should drive both repair behavior and observability.

## Safety Invariants

The auto-repair path must preserve these invariants:

1. Never generate missing orders just because startup found a mismatch.
2. Never combine orphan fragments across strategies.
3. Never apply the same `trade_id` twice.
4. Never auto-repair if two different explanations remain plausible.
5. After repair, position reconciliation must still rerun and succeed against venue truth before startup completes.

## Observability

Startup repairs must be explicit and auditable.

Required additions:
- publish structured startup repair alerts with `cause`, `action`, `cached_qty`, `venue_qty`, affected order ids, and affected position ids
- add counters for:
  - `startup_auto_repair_stale_positions`
  - `startup_auto_repair_missing_orders`
  - `startup_auto_repair_orphan_lineage`
  - `startup_auto_repair_duplicate_fill_skip`
  - `startup_auto_repair_ambiguous_failure`
- log one compact per-instrument startup summary with snapshot qty, venue qty, fill count, startup open-order count, repair action, and final result

## TokenMM Readiness

TokenMM readiness currently hides these Binance failures because `profile=tokenmm` only requires a subset of venues.

After the engine fix is validated, the rollout should also update [tokenmm.live.toml](/home/ubuntu/nautilus_trader/deploy/tokenmm/tokenmm.live.toml) so the Binance strategies contribute to required readiness for the production TokenMM profile.

That is a rollout hardening step, not a prerequisite for the engine fix itself.

## Testing

Required regression coverage:

1. Binance spot shape:
   - cached short position
   - one cached startup open order
   - venue flat at startup
   - recent startup fills present
   - auto-repair should reject the stale order, close the stale position, and succeed

2. Binance perp shape:
   - cached qty already matches venue before startup
   - startup fills and order history are present
   - startup must not finish with local qty `+1000` over venue

3. Grouped orphan lineage:
   - exact same-strategy bounded subset restore succeeds
   - ambiguous multi-fragment combinations still fail closed

4. Existing strict-failure cases remain strict when the evidence is ambiguous.

## Rollout

1. Add the focused unit regressions first.
2. Implement the bounded startup auto-repair path.
3. Run the targeted live execution reconciliation suite.
4. Verify no existing strict-failure tests regress.
5. Update TokenMM readiness config to include the Binance strategies.
6. Restart the affected Binance TokenMM services and verify:
   - startup completes
   - repair action, if any, is logged and counted
   - readiness reports the strategies accurately

## Long-Term Hardening

The longer-term design should reduce dependence on serialized `Position` objects as startup truth.

Follow-up direction:
- treat persisted positions as denormalized cache, not primary state
- persist a startup lineage index keyed by `trade_id`, `client_order_id`, `venue_order_id`, and `strategy_id`
- derive effective startup net qty from lineage first and cached positions second

That is a separate project. It should not block the bounded auto-repair fix.

## Non-Goals

- enabling `generate_missing_orders` for this incident class
- replacing fail-closed startup behavior with unconditional venue-truth rebuilds
- broad adapter-specific logic for Binance only
- a full cache storage redesign in this remediation PR

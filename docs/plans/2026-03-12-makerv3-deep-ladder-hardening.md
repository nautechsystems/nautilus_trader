# MakerV3 Deep Ladder Hardening Implementation Plan

> **For the execution agent:** REQUIRED SUB-SKILL: Before implementing this plan, choose exactly one execution mode and use the matching skill: `superpowers:subagent-driven-development` for same-session execution or `superpowers:executing-plans` for a separate-session handoff.
> **Progress:** The Progress Tracker in this document is the execution source of truth and must be updated on every state change.

**Goal:** Make MakerV3 production-safe for deep three-band ladders on Bybit/OKX/Bitget by replacing unbounded cancel/replace behavior with a bounded, venue-aware, low-latency rebalance model while preserving existing safety circuits.

**Architecture:** Keep the current split between pricing, pure rebalancing, quote orchestration, and venue-protection, but change the side planner from "cancel everything that is now theoretically wrong" to "converge toward the target ladder with bounded deltas per cycle." State publishing and managed-order tracking must also be made snapshot-safe and terminally correct so the UI and operators see truthful stop/block states under stress.

**Tech Stack:** Python, Nautilus Trader/Flux, Redis strategy state payloads, pytest unit tests for MakerV3/Flux API.

## Progress Tracker

**Source of truth:** Update this table whenever task state changes. Do not rely on memory, chat history, or TodoWrite alone.

| Task | Status | Owner | Depends On | Write Scope | Lane Branch | Worktree Path | Commit / Diff | Verification | Notes / Last Update |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Overall | not_started | unassigned | none | `systems/flux/flux/strategies/makerv3`, `systems/flux/flux/api`, `tests/unit_tests/flux/strategies/makerv3`, `tests/unit_tests/flux/api`, `docs/plans/2026-03-12-makerv3-deep-ladder-hardening.md` | `codex/bybit-makerv3-ladder-hardening-20260312` | `/home/ubuntu/nautilus_trader/.worktrees/codex-bybit-makerv3-ladder-hardening-20260312` | none | baseline_env_missing_native_ext | Plan created in isolated worktree; local worktree pytest currently fails to import Nautilus native modules until the extension build is available in this checkout. |
| Task 1: Capture the current failure mode with characterization tests | not_started | unassigned | none | `tests/unit_tests/flux/strategies/makerv3/test_rebalancing.py`, `tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py`, `tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py`, `tests/unit_tests/flux/strategies/makerv3/test_order_safety.py` | `shared` | `shared` | none | not_run | Plan created |
| Task 2: Make state publishing snapshot-safe under cancel storms | not_started | unassigned | Task 1: Capture the current failure mode with characterization tests | `systems/flux/flux/strategies/makerv3/strategy.py`, `systems/flux/flux/strategies/makerv3/publisher.py`, `tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py`, `tests/unit_tests/flux/api/test_app.py` | `shared` | `shared` | none | not_run | Plan created |
| Task 3: Replace unbounded side rebalancing with bounded deep-ladder convergence | not_started | unassigned | Task 1: Capture the current failure mode with characterization tests | `systems/flux/flux/strategies/makerv3/rebalancing.py`, `systems/flux/flux/strategies/makerv3/strategy.py`, `systems/flux/flux/strategies/makerv3/quote_engine.py`, `systems/flux/flux/strategies/makerv3/pricing.py`, `tests/unit_tests/flux/strategies/makerv3/test_rebalancing.py`, `tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py` | `shared` | `shared` | none | not_run | Plan created |
| Task 4: Add venue-aware pacing and preserve latency/performance invariants | not_started | unassigned | Task 3: Replace unbounded side rebalancing with bounded deep-ladder convergence | `systems/flux/flux/strategies/makerv3/strategy.py`, `systems/flux/flux/strategies/makerv3/runtime_params.py`, `systems/flux/flux/strategies/makerv3/quote_engine.py`, `tests/unit_tests/flux/strategies/makerv3/test_runtime_params.py`, `tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py` | `shared` | `shared` | none | not_run | Plan created |
| Task 5: Reconcile terminal cancel rejects into truthful managed-order state | not_started | unassigned | Task 1: Capture the current failure mode with characterization tests | `systems/flux/flux/strategies/makerv3/strategy.py`, `systems/flux/flux/strategies/makerv3/managed_orders.py`, `tests/unit_tests/flux/strategies/makerv3/test_order_safety.py`, `tests/unit_tests/flux/strategies/makerv3/test_managed_orders.py` | `shared` | `shared` | none | not_run | Plan created |
| Task 6: Validate external behavior and document rollout guidance | not_started | unassigned | Task 2: Make state publishing snapshot-safe under cancel storms, Task 3: Replace unbounded side rebalancing with bounded deep-ladder convergence, Task 4: Add venue-aware pacing and preserve latency/performance invariants, Task 5: Reconcile terminal cancel rejects into truthful managed-order state | `tests/unit_tests/flux/api/test_app.py`, `tests/unit_tests/flux/api/test_payloads.py`, `docs/plans/2026-03-12-makerv3-deep-ladder-hardening.md` | `shared` | `shared` | none | not_run | Plan created |

---

## External Review Context

### Current Behavior

MakerV3 currently computes a full theoretical ladder every quote cycle in [quote_engine.py](/home/ubuntu/nautilus_trader/systems/flux/flux/strategies/makerv3/quote_engine.py), then asks the side planner in [rebalancing.py](/home/ubuntu/nautilus_trader/systems/flux/flux/strategies/makerv3/rebalancing.py) which orders to cancel and which levels are missing. The hot path is:

1. Build desired place/cancel ladders from fair-value/reference prices in [pricing.py](/home/ubuntu/nautilus_trader/systems/flux/flux/strategies/makerv3/pricing.py)
2. Sort active orders aggressive-to-passive in [quote_engine.py](/home/ubuntu/nautilus_trader/systems/flux/flux/strategies/makerv3/quote_engine.py)
3. Derive cancel actions in [rebalancing.py](/home/ubuntu/nautilus_trader/systems/flux/flux/strategies/makerv3/rebalancing.py)
4. Send all resulting cancels immediately in [_rebalance_side()](/home/ubuntu/nautilus_trader/systems/flux/flux/strategies/makerv3/strategy.py#L2286)
5. If no pending cancels remain, place all missing levels

The problem is that only stale cancels are budgeted. `too_aggressive` cancels and `free_slot_for_missing_level` cancels are unbounded within a cycle. That means a deep ladder can legitimately decide to cancel a large fraction of one side at once when the desired ladder widens or shifts. On Bybit, that becomes a cancel storm rather than controlled convergence.

### Observed Failure Mode

The live Bybit perp failure on `2026-03-12` was not a startup-reconciliation error. It was a runtime quote-management failure:

- Live params had a deep ladder shape (`n_orders1=5`, `n_orders2=5`) and tight place/cancel hysteresis.
- The strategy emitted large submit/cancel bursts in the same quoting window.
- Bybit replied with many `order not exists or too late to cancel` rejects, then `Too many visits. Exceeded the API Rate Limit.`
- Venue protection correctly hard-stopped the strategy in [failures.py](/home/ubuntu/nautilus_trader/systems/flux/flux/strategies/makerv3/failures.py).

Two secondary defects made the operator view worse:

- State publishing crashed during the cancel storm because [_quote_progress_payload()](/home/ubuntu/nautilus_trader/systems/flux/flux/strategies/makerv3/strategy.py#L904) iterates a mutable pending-cancel set, which can raise `RuntimeError: Set changed size during iteration` via [publisher.py](/home/ubuntu/nautilus_trader/systems/flux/flux/strategies/makerv3/publisher.py).
- `on_order_cancel_rejected()` in [strategy.py](/home/ubuntu/nautilus_trader/systems/flux/flux/strategies/makerv3/strategy.py#L1240) clears pending-cancel state but does not reconcile the tracked managed-order set for terminal-ish cancel rejects, so tracked IDs can diverge from cache truth.

### Gaps

1. **Algorithmic gap:** The rebalancer is theoretically correct but operationally unsafe for deep ladders on rate-limited venues. It converges too quickly and without a per-cycle replace budget.
2. **Venue-fit gap:** There is no explicit venue-aware rate-of-change policy. MakerV3 knows what prices it wants, but not how fast each venue can safely move there.
3. **State-truth gap:** Under stress, state publishing can fail and stale Redis state later masquerades as a dead runner in [app.py](/home/ubuntu/nautilus_trader/systems/flux/flux/api/app.py).
4. **Tracking gap:** Terminal cancel rejects do not fully reconcile managed-order tracking, so the internal order view becomes noisier over time.

### Performance And Latency Constraints

The replacement design must stay HFT-standard:

- no extra network round trips in the hot path
- no database or Redis reads added to the quote-cycle decision loop
- pure planner logic stays local and deterministic
- bounded work per side per cycle
- preserve current low-latency pricing path and existing market-data freshness gates
- preserve venue protection as the last-resort safety stop, not the primary traffic-shaping mechanism

## Design Options

### Option A: Parameter-only mitigation

Reduce depth, widen edges, or relax `max_age_ms` on Bybit. This is useful as an emergency runbook but not an acceptable design if the long-term requirement is three live bands and `15+` orders per side.

**Verdict:** Reject as the primary fix. Keep only as temporary operator mitigation.

### Option B: Keep the current planner and add crude global throttles

Add a top-level "don’t send more than N cancels per second" guard while leaving the side planner unchanged.

**Verdict:** Reject. This hides the symptom but leaves the planner semantically wrong. The strategy would still decide to replace the whole ladder and then silently fail to converge.

### Option C: Bounded convergence with venue-aware change budgets

Preserve the current pricing math, but change rebalancing semantics so the ladder converges incrementally. Aggressive edge changes should peel from the touch outward, passive overflow should peel from the tail inward, and each side should have an explicit change budget per cycle. This keeps deep ladders live while matching venue constraints.

**Verdict:** Recommended.

## Recommended Design

Introduce a two-stage rebalance model:

1. **Target generation:** keep the existing ladder generation and skew logic
2. **Bounded side convergence:** replace the current "all cancel actions now" planner with a pure function that:
   - classifies active orders as `keep`, `replace_wider`, `replace_tighter`, `drop_tail`, `stale_tail`
   - prioritizes the smallest safe set of changes needed to improve the book
   - enforces a per-side change budget for cancels and places
   - never treats a deep-ladder widening move as permission to flush the whole side in one cycle

Additional rules:

- most aggressive wrong orders get first priority when widening a side
- least aggressive orders get first priority when freeing slots or aging out
- interior levels that are still acceptable should be preserved even if the theoretical target ladder has shifted
- cancel rejects that mean "the order is already gone" must reconcile tracking immediately
- quote/state export paths must use immutable snapshots so observability cannot fail during a stress event

## Non-Goals

- Removing or weakening venue protection
- Permanently reducing supported ladder depth to one band
- Adding exchange-specific code forks for every venue in the hot path
- Replacing the current pricing/skew model

## Task 1: Capture the current failure mode with characterization tests

**Files:**
- Modify: `tests/unit_tests/flux/strategies/makerv3/test_rebalancing.py`
- Modify: `tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py`
- Modify: `tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py`
- Modify: `tests/unit_tests/flux/strategies/makerv3/test_order_safety.py`

**Dependencies:** `none`

**Write Scope:** `tests/unit_tests/flux/strategies/makerv3/test_rebalancing.py`, `tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py`, `tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py`, `tests/unit_tests/flux/strategies/makerv3/test_order_safety.py`

**Verification Commands:**
- `pytest -q tests/unit_tests/flux/strategies/makerv3/test_rebalancing.py`
- `pytest -q tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py -k 'pending_cancel or rebalanced'`
- `pytest -q tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py -k 'pending_cancel or quote_progress'`
- `pytest -q tests/unit_tests/flux/strategies/makerv3/test_order_safety.py -k 'cancel_rejected or venue_protection'`

**Step 1: Write the failing planner characterization tests**

Add tests that pin the current unsafe cases:

- widening one side of a deep ladder currently schedules many `too_aggressive` cancels at once
- a one-tick/two-tick ladder shift with deep bands can request more cancels than a venue-safe cycle should allow
- the planner does not preserve a large interior overlap when only the touch should change

**Step 2: Write the failing quote-cycle tests**

Add tests that pin:

- quote cycles with many pending cancels should not continue into broad replace waves
- state export during heavy pending-cancel mutation must not raise
- venue-protection stop should leave a truthful terminal state payload rather than relying on staleness

**Step 3: Run the targeted tests and record the failures**

Run the commands above and capture the exact failing assertions that describe the undesired behavior.

**Step 4: Commit the red characterization tests**

```bash
git add tests/unit_tests/flux/strategies/makerv3/test_rebalancing.py \
        tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py \
        tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py \
        tests/unit_tests/flux/strategies/makerv3/test_order_safety.py
git commit -m "test: characterize deep ladder churn failure modes"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

## Task 2: Make state publishing snapshot-safe under cancel storms

**Files:**
- Modify: `systems/flux/flux/strategies/makerv3/strategy.py`
- Modify: `systems/flux/flux/strategies/makerv3/publisher.py`
- Modify: `tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py`
- Modify: `tests/unit_tests/flux/api/test_app.py`

**Dependencies:** `Task 1: Capture the current failure mode with characterization tests`

**Write Scope:** `systems/flux/flux/strategies/makerv3/strategy.py`, `systems/flux/flux/strategies/makerv3/publisher.py`, `tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py`, `tests/unit_tests/flux/api/test_app.py`

**Verification Commands:**
- `pytest -q tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py`
- `pytest -q tests/unit_tests/flux/api/test_app.py -k 'running_state or state_summary or signals'`

**Step 1: Write a failing test for mutable pending-cancel state export**

Add a test that mutates pending-cancel tracking while state/quote-progress payloads are being constructed and assert no exception is raised.

**Step 2: Implement immutable snapshot reads**

Change `_quote_progress_payload()` and any related payload helpers to snapshot pending-cancel IDs and timestamps before iterating. Avoid mutation-while-iterating hazards without adding extra hot-path allocations beyond one bounded local snapshot.

**Step 3: Write and run app-level stale-state tests**

Add tests showing that a strategy which explicitly publishes a terminal stop/block state remains legible to the API, and that stale-state fallback is only used when no terminal state was published.

**Step 4: Commit**

```bash
git add systems/flux/flux/strategies/makerv3/strategy.py \
        systems/flux/flux/strategies/makerv3/publisher.py \
        tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py \
        tests/unit_tests/flux/api/test_app.py
git commit -m "fix: make makerv3 state exports snapshot safe"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

## Task 3: Replace unbounded side rebalancing with bounded deep-ladder convergence

**Files:**
- Modify: `systems/flux/flux/strategies/makerv3/rebalancing.py`
- Modify: `systems/flux/flux/strategies/makerv3/strategy.py`
- Modify: `systems/flux/flux/strategies/makerv3/quote_engine.py`
- Modify: `systems/flux/flux/strategies/makerv3/pricing.py`
- Modify: `tests/unit_tests/flux/strategies/makerv3/test_rebalancing.py`
- Modify: `tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py`

**Dependencies:** `Task 1: Capture the current failure mode with characterization tests`

**Write Scope:** `systems/flux/flux/strategies/makerv3/rebalancing.py`, `systems/flux/flux/strategies/makerv3/strategy.py`, `systems/flux/flux/strategies/makerv3/quote_engine.py`, `systems/flux/flux/strategies/makerv3/pricing.py`, `tests/unit_tests/flux/strategies/makerv3/test_rebalancing.py`, `tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py`

**Verification Commands:**
- `pytest -q tests/unit_tests/flux/strategies/makerv3/test_rebalancing.py`
- `pytest -q tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py`

**Step 1: Write failing planner tests for bounded convergence**

Add tests that define the desired semantics:

- widening a side only cancels the worst aggressors up to budget
- tightening a side only frees passive tail slots up to budget
- preserved overlap stays in place even when target levels move
- a deep three-band ladder converges over multiple cycles instead of one full refresh

**Step 2: Introduce a richer pure planner result**

Evolve the rebalancing planner from raw cancel indices into a structured plan that distinguishes:

- `replace_aggressive`
- `drop_tail`
- `stale_tail`
- `missing_levels_to_place_now`
- `deferred_levels`

Keep it pure and bounded. The planner must remain O(n) or O(n log n) on already-sorted side inputs and avoid venue/API knowledge.

**Step 3: Integrate the bounded plan into the quote engine**

Use the structured planner result in the quote cycle so cancels and places are budgeted per side and deferred cleanly across cycles.

**Step 4: Preserve latency invariants**

Ensure the hot path still:

- builds the ladder once
- sorts active orders once
- runs one pure side planner per side
- emits only a bounded number of command objects per cycle

**Step 5: Commit**

```bash
git add systems/flux/flux/strategies/makerv3/rebalancing.py \
        systems/flux/flux/strategies/makerv3/strategy.py \
        systems/flux/flux/strategies/makerv3/quote_engine.py \
        systems/flux/flux/strategies/makerv3/pricing.py \
        tests/unit_tests/flux/strategies/makerv3/test_rebalancing.py \
        tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py
git commit -m "feat: bound makerv3 deep ladder convergence"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

## Task 4: Add venue-aware pacing and preserve latency/performance invariants

**Files:**
- Modify: `systems/flux/flux/strategies/makerv3/strategy.py`
- Modify: `systems/flux/flux/strategies/makerv3/runtime_params.py`
- Modify: `systems/flux/flux/strategies/makerv3/quote_engine.py`
- Modify: `tests/unit_tests/flux/strategies/makerv3/test_runtime_params.py`
- Modify: `tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py`

**Dependencies:** `Task 3: Replace unbounded side rebalancing with bounded deep-ladder convergence`

**Write Scope:** `systems/flux/flux/strategies/makerv3/strategy.py`, `systems/flux/flux/strategies/makerv3/runtime_params.py`, `systems/flux/flux/strategies/makerv3/quote_engine.py`, `tests/unit_tests/flux/strategies/makerv3/test_runtime_params.py`, `tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py`

**Verification Commands:**
- `pytest -q tests/unit_tests/flux/strategies/makerv3/test_runtime_params.py`
- `pytest -q tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py -k 'budget or pending_cancel or cooldown'`

**Step 1: Define runtime surfaces for change budgeting**

Add explicit runtime params for bounded convergence only if they are truly required. Prefer a minimal surface such as:

- max cancels per side per cycle
- max places per side per cycle
- optional venue profile / defaults source

Do not expose a large tuning matrix if the values can be internal policy.

**Step 2: Write failing quote-engine tests**

Add tests showing that:

- Bybit-like policy does not allow full-side replacement in a single cycle
- deferred work is retried on subsequent cycles
- pending-cancel pressure blocks additional broad churn

**Step 3: Implement venue-aware defaults**

Keep venue protection as the backstop, but make the normal quote loop rate-aware enough that Bybit should not hit protection under ordinary deep-ladder operation.

**Step 4: Commit**

```bash
git add systems/flux/flux/strategies/makerv3/strategy.py \
        systems/flux/flux/strategies/makerv3/runtime_params.py \
        systems/flux/flux/strategies/makerv3/quote_engine.py \
        tests/unit_tests/flux/strategies/makerv3/test_runtime_params.py \
        tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py
git commit -m "feat: add venue-aware makerv3 pacing budgets"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

## Task 5: Reconcile terminal cancel rejects into truthful managed-order state

**Files:**
- Modify: `systems/flux/flux/strategies/makerv3/strategy.py`
- Modify: `systems/flux/flux/strategies/makerv3/managed_orders.py`
- Modify: `tests/unit_tests/flux/strategies/makerv3/test_order_safety.py`
- Modify: `tests/unit_tests/flux/strategies/makerv3/test_managed_orders.py`

**Dependencies:** `Task 1: Capture the current failure mode with characterization tests`

**Write Scope:** `systems/flux/flux/strategies/makerv3/strategy.py`, `systems/flux/flux/strategies/makerv3/managed_orders.py`, `tests/unit_tests/flux/strategies/makerv3/test_order_safety.py`, `tests/unit_tests/flux/strategies/makerv3/test_managed_orders.py`

**Verification Commands:**
- `pytest -q tests/unit_tests/flux/strategies/makerv3/test_order_safety.py`
- `pytest -q tests/unit_tests/flux/strategies/makerv3/test_managed_orders.py`

**Step 1: Write failing tests for terminal-ish cancel rejects**

Add tests for:

- `order not exists or too late to cancel`
- unknown-order/state-mismatch equivalents

The strategy-level expectation is that pending-cancel state clears and tracked managed-order state reconciles when the venue indicates the order is already gone.

**Step 2: Implement tracked-order reconciliation**

Add a narrow helper for cancel-reject reconciliation that only fires for reasons proven to mean terminal absence. Do not generalize all cancel rejects into terminal closures.

**Step 3: Verify venue protection still fires correctly**

Keep the existing rate-limit and order-limit safety behavior intact. This task must not dilute `venue_protection_circuit_breaker`.

**Step 4: Commit**

```bash
git add systems/flux/flux/strategies/makerv3/strategy.py \
        systems/flux/flux/strategies/makerv3/managed_orders.py \
        tests/unit_tests/flux/strategies/makerv3/test_order_safety.py \
        tests/unit_tests/flux/strategies/makerv3/test_managed_orders.py
git commit -m "fix: reconcile terminal cancel rejects in makerv3"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

## Task 6: Validate external behavior and document rollout guidance

**Files:**
- Modify: `tests/unit_tests/flux/api/test_app.py`
- Modify: `tests/unit_tests/flux/api/test_payloads.py`
- Modify: `docs/plans/2026-03-12-makerv3-deep-ladder-hardening.md`

**Dependencies:** `Task 2: Make state publishing snapshot-safe under cancel storms`, `Task 3: Replace unbounded side rebalancing with bounded deep-ladder convergence`, `Task 4: Add venue-aware pacing and preserve latency/performance invariants`, `Task 5: Reconcile terminal cancel rejects into truthful managed-order state`

**Write Scope:** `tests/unit_tests/flux/api/test_app.py`, `tests/unit_tests/flux/api/test_payloads.py`, `docs/plans/2026-03-12-makerv3-deep-ladder-hardening.md`

**Verification Commands:**
- `pytest -q tests/unit_tests/flux/api/test_app.py`
- `pytest -q tests/unit_tests/flux/api/test_payloads.py`
- `pytest -q tests/unit_tests/flux/strategies/makerv3`

**Step 1: Add API/payload regression coverage**

Add tests that prove:

- stopped/blocked strategies remain operator-visible without relying on stale-state inference
- quote progress/blocker payloads remain stable during pending-cancel pressure

**Step 2: Record rollout and rollback guidance in this plan**

Document:

- recommended Bybit canary parameters during rollout
- expected metrics and logs
- rollback condition
- how to distinguish venue-protection safety stops from startup failures or dead runners

**Step 3: Run the full touched test surface**

Run the commands above plus any additional focused suites touched during implementation.

**Step 4: Commit**

```bash
git add tests/unit_tests/flux/api/test_app.py \
        tests/unit_tests/flux/api/test_payloads.py \
        docs/plans/2026-03-12-makerv3-deep-ladder-hardening.md
git commit -m "docs: finalize makerv3 deep ladder hardening rollout plan"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

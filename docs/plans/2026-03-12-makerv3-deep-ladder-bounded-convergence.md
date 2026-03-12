# MakerV3 Deep Ladder Bounded Convergence Implementation Plan

> **For the execution agent:** REQUIRED SUB-SKILL: Before implementing this plan, choose exactly one execution mode and use the matching skill: `superpowers:subagent-driven-development` for same-session execution or `superpowers:executing-plans` for a separate-session handoff.
> **Progress:** The Progress Tracker in this document is the execution source of truth and must be updated on every state change.

**Goal:** Make MakerV3 safe and efficient for deep 3-band ladders on Bybit, OKX, and Bitget by replacing unbounded cancel/replace behavior with venue-aware bounded convergence while preserving low-latency quote maintenance.

**Architecture:** Keep pricing and target generation pure, but replace the current per-side rebalance planner with a deterministic bounded-convergence planner that separates target computation from action budgeting. The strategy should converge toward the desired ladder incrementally under explicit per-side and per-venue budgets, with correctness fixes for pending-cancel state, cancel-reject reconciliation, and terminal-state publishing. The first implementation should keep the control surface intentionally small and portable: simple budgets, explicit action priorities, and no venue-specific hot-path branches.

**Tech Stack:** Python, Nautilus Trader strategy runtime, Flux MakerV3, Redis state publishing, pytest unit tests.

## External Review Context

### Current Intended Semantics

MakerV3 should support deep ladders without full-book churn. In steady state, the quote loop should mostly no-op. When the market tightens, the strategy should add at the touch and peel passive tail orders only as needed. When the market widens, it should cancel the most aggressive orders first and converge outward under a bounded action budget. This is the expected HFT-standard shape for a deep resting stack: high quote quality, low unnecessary churn, deterministic change-rate control, and venue-fit rate behavior.

### Current Observed Behavior

On Thursday, March 12, 2026 at `09:44:06 UTC`, `plumeusdt_bybit_perp_makerv3` self-stopped under venue protection after a cancel storm. The relevant evidence from the current code/log path is:

- Live runtime params on Bybit perp were deep and tight: `n_orders1=5`, `n_orders2=5`, `n_orders3=0`, which is a 10-level ladder per side.
- During the failure window, the strategy emitted bursts consistent with large-scale side replacement, culminating in `cache_count=21`, `tracked_count=75`, and a `venue_protection_circuit_breaker` stop.
- Bybit returned repeated `order not exists or too late to cancel` rejects followed by `Too many visits. Exceeded the API Rate Limit.`.
- The process remained alive, but state publishing later went stale, so Flux displayed a misleading `Runner Off` state.

### Current Code Paths And Gaps

1. **Unbounded aggressive-side cancellation**

   The current planner in `systems/flux/flux/strategies/makerv3/rebalancing.py` only budgets `stale` cancels. `too_aggressive` cancels and `free_slot_for_missing_level` cancels are effectively unbounded. `_rebalance_side()` in `systems/flux/flux/strategies/makerv3/strategy.py` then emits all cancel actions in one pass.

   Consequence: a widening move can mark many orders as too aggressive and attempt to cancel an entire side in one cycle.

2. **Deep ladders are treated as an unconstrained convergence problem**

   `systems/flux/flux/strategies/makerv3/quote_engine.py` computes the full desired ladder each cycle and then applies the full rebalance result immediately. There is no venue-aware rate budget, no per-side cancel/replace cap, and no distinction between "desired final ladder" and "allowed movement this cycle".

   Consequence: deep ladders are safe only when the market is quiet. Under movement, theoretical ladder convergence can exceed venue-safe change rates.

3. **State publish path is not safe under cancel storms**

   `_quote_progress_payload()` in `systems/flux/flux/strategies/makerv3/strategy.py` iterates `self._pending_cancel_client_order_ids` directly. During the Bybit failure window this threw `RuntimeError: Set changed size during iteration`, which broke state publishing during a bursty cancel phase.

   Consequence: terminal or blocked strategy states can be lost, and Flux falls back to stale-state heuristics.

4. **Cancel rejects do not fully reconcile strategy-managed tracking**

   `on_order_cancel_rejected()` clears pending-cancel state but does not reconcile the strategy-managed order tracker for terminal-ish venue reasons such as `order not exists or too late to cancel`.

   Consequence: tracked managed-order counts can drift away from cache truth, making cleanup and observability noisier and increasing recovery fragility.

5. **Flux UI semantics degrade when the latest terminal state is not published**

   Flux API liveness in `systems/flux/flux/api/app.py` marks non-`on_stop` stale states as not running and drops the summary payload. This is reasonable when state publishing is healthy, but it becomes misleading when state export itself fails mid-stop.

   Consequence: the UI says `Runner Off` when the runner process is still alive but the strategy is stopped inside it.

### Design Requirements

- Support steady-state 3-band ladders with `15+` orders per side.
- Preserve low latency and avoid heavy per-cycle object churn.
- Constrain cancel/replace rate explicitly per side and per venue.
- Keep the pricing path pure and deterministic.
- Avoid venue-specific spaghetti in core rebalance logic.
- Preserve quote quality while making venue protection exceptional, not routine.
- Distinguish healthy bounded convergence from persistent lag and from broken state.

### Non-Goals

- Do not solve Binance-specific issues in this wave.
- Do not change the economic ladder model, skew model, or pricing anchors unless required by bounded convergence.
- Do not hack around the problem by permanently reducing Bybit to a shallow ladder. Temporary rollout profiles may be used, but the architecture must support deep ladders correctly.
- Do not introduce a large matrix of venue- and band-specific pacing knobs in v1.

## Proposed Architecture

### 1. Split "desired ladder" from "allowed action budget"

Keep the current pricing/target generation responsibilities in `pricing.py` and `quote_engine.py`, but stop treating the full desired ladder as an immediate execution plan. Introduce a pure bounded-convergence planner that accepts:

- current active orders for one side
- desired ladder levels for that side
- stale flags
- pending-cancel / in-flight state
- venue action budget for this cycle

The planner should return a small, ordered action set:

- `cancel` actions, prioritized by safety and venue fit
- `place` actions, bounded by remaining slots and action budget
- explicit convergence diagnostics for telemetry

### 1a. Planner Contract Must Stay Pure

The planner should be a pure side-local function with an explicit contract.

**Inputs**

- ordered active managed orders or order descriptors for one side
- ordered desired levels for one side
- stale classification per active level
- pending-backlog classification as explicit input flags or counters
- budget:
  - max cancels
  - max places
  - max total actions

**Outputs**

- ordered cancel actions
- ordered place actions or desired level indices
- diagnostics:
  - why each action was chosen
  - how many desired levels remain missing
  - whether budget blocked more work
  - whether backlog gating prevented more work

**Constraints**

- no cache reads
- no venue objects
- no hidden mutation
- no internal wall-clock logic beyond explicit inputs
- no direct strategy or Redis access

This is important because otherwise the planner will collapse back into `strategy.py` logic in another file.

### 2. Converge directionally, not globally

The core rebalancing policy for a deep ladder should be:

- **Excess passive tail**: cancel from the least aggressive end first.
- **More aggressive missing levels**: create room by peeling the passive tail, not by broad ladder replacement.
- **Widening moves**: cancel the most aggressive orders first, but under a small per-cycle cap.
- **Stale cleanup**: keep a separate stale budget from active repricing budget.
- **Pending cancel saturation**: once cancel backlog is present, move through explicit throttle/freeze/block states instead of continuing normal repricing.

This keeps the ladder elegant: it behaves like a bounded moving front rather than a full resnapshot.

### 2a. Explicit Room-Creation Rules

When side capacity is full and more aggressive missing levels need room, the planner should prefer:

1. cancel excess passive tail beyond desired depth
2. cancel stale orders already needing replacement
3. peel the least aggressive in-range order only when needed to create one slot
4. never broad-cancel multiple in-range orders in one cycle just because several top levels are missing

That last rule is the main defense against pseudo-resnapshot behavior under oscillating markets.

### 2b. Add Hysteresis / Keep Bucket

Budgets prevent catastrophe, but they do not by themselves prevent constant low-value churn. The planner should classify active levels into:

- `must_cancel`
- `should_cancel_if_budget_allows`
- `keep`

This gives the ladder stickiness. A level that is merely suboptimal but still safe should usually remain in place, especially in the passive outer bands.

### 3. Make venue pacing explicit

Add venue-aware runtime/config budgets that control:

- max cancels per side per quote cycle
- max places per side per quote cycle
- max total change actions per cycle
- max pending cancels per side before throttling

These should live in MakerV3 runtime params/config, but the policy logic should stay in the pure planner layer so the fast path remains deterministic and testable.

The initial implementation should stop there. Do not add band-specific budgets, adaptive rate matrices, or venue-specific action taxonomies unless the simple version proves insufficient.

### 3a. Backlog Modes Must Be Explicit

Backlog handling should distinguish:

- **soft throttle**: reduce or suspend repricing while allowing minimal safe maintenance
- **hard freeze**: no new repricing on that side until backlog drains
- **blocked**: pathological backlog age/size, explicit operator-visible blocked state

Age matters as much as count. Diagnostics and policy should use:

- `pending_cancel_count`
- `oldest_pending_cancel_age_ms`
- repeated cancel-reject count by reason

This lets operators and the strategy distinguish brief exchange lag from true desync or pathological venue state.

### 4. Harden correctness around cancel lifecycle and state export

The convergence change should ship with the required correctness fixes:

- snapshot pending-cancel collections before export/iteration
- reconcile tracked managed orders for terminal-ish cancel rejects
- ensure terminal blocked/stopped state survives venue-protection stops

These are not optional cleanups; they are part of making the convergence engine auditable and production-safe.

### 5. Make Convergence Observable

Bounded convergence means the ladder may be intentionally not-perfect for short periods. Telemetry must distinguish healthy lag from broken state.

At minimum, expose:

- desired levels count
- matched active levels count
- missing aggressive levels count
- excess passive levels count
- actions skipped due to budget
- backlog throttle mode
- convergence lag indicator

Operators should be able to tell the difference between:

- healthy bounded convergence
- persistent lag under budget pressure
- pathological blocked/stopped state

## Progress Tracker

**Source of truth:** Update this table whenever task state changes. Do not rely on memory, chat history, or TodoWrite alone.

| Task | Status | Owner | Depends On | Write Scope | Lane Branch | Worktree Path | Commit / Diff | Verification | Notes / Last Update |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Task 1: Reproduce Failure Mode And Lock Spec Tests | not_started | unassigned | none | `tests/unit_tests/flux/strategies/makerv3/test_rebalancing.py`, `tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py`, `tests/unit_tests/flux/strategies/makerv3/test_order_safety.py`, `tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py` | `shared` | `/home/ubuntu/nautilus_trader/.worktrees/codex-bybit-makerv3-ladder-hardening-20260312` | none | not_run | Plan created |
| Task 2: Introduce Pure Bounded-Convergence Planner | not_started | unassigned | Task 1: Reproduce Failure Mode And Lock Spec Tests | `systems/flux/flux/strategies/makerv3/rebalancing.py`, `tests/unit_tests/flux/strategies/makerv3/test_rebalancing.py` | `shared` | `/home/ubuntu/nautilus_trader/.worktrees/codex-bybit-makerv3-ladder-hardening-20260312` | none | not_run | Plan created |
| Task 3: Integrate Planner Into Quote Cycle With Venue Budgets | not_started | unassigned | Task 2: Introduce Pure Bounded-Convergence Planner | `systems/flux/flux/strategies/makerv3/strategy.py`, `systems/flux/flux/strategies/makerv3/quote_engine.py`, `systems/flux/flux/strategies/makerv3/runtime_params.py`, `systems/flux/flux/common/params.py`, `tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py`, `tests/unit_tests/flux/strategies/makerv3/test_runtime_params.py` | `shared` | `/home/ubuntu/nautilus_trader/.worktrees/codex-bybit-makerv3-ladder-hardening-20260312` | none | not_run | Plan created |
| Task 4: Fix Cancel-Reject Reconciliation And State Export Correctness | not_started | unassigned | Task 1: Reproduce Failure Mode And Lock Spec Tests | `systems/flux/flux/strategies/makerv3/strategy.py`, `systems/flux/flux/strategies/makerv3/publisher.py`, `tests/unit_tests/flux/strategies/makerv3/test_order_safety.py`, `tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py`, `tests/unit_tests/flux/api/test_app.py` | `shared` | `/home/ubuntu/nautilus_trader/.worktrees/codex-bybit-makerv3-ladder-hardening-20260312` | none | not_run | Plan created |
| Task 5: Rollout Profiles, Telemetry, And Verification | not_started | unassigned | Task 4: Fix Cancel-Reject Reconciliation And State Export Correctness | `deploy/tokenmm/strategies/plumeusdt_bybit_perp_makerv3.toml`, `deploy/tokenmm/strategies/plumeusdt_okx_perp_makerv3.toml`, `deploy/tokenmm/strategies/plumeusdt_bitget_perp_makerv3.toml`, `docs/runbooks`, `tests/unit_tests/flux/strategies/makerv3/*`, `tests/unit_tests/flux/api/*` | `shared` | `/home/ubuntu/nautilus_trader/.worktrees/codex-bybit-makerv3-ladder-hardening-20260312` | none | not_run | Plan created |

---

### Task 1: Reproduce Failure Mode And Lock Spec Tests

**Files:**
- Modify: `tests/unit_tests/flux/strategies/makerv3/test_rebalancing.py`
- Modify: `tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py`
- Modify: `tests/unit_tests/flux/strategies/makerv3/test_order_safety.py`
- Modify: `tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py`

**Dependencies:** `none`

**Write Scope:** `tests/unit_tests/flux/strategies/makerv3/test_rebalancing.py`, `tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py`, `tests/unit_tests/flux/strategies/makerv3/test_order_safety.py`, `tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py`

**Verification Commands:**
- `pytest -q tests/unit_tests/flux/strategies/makerv3/test_rebalancing.py`
- `pytest -q tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py -k 'pending_cancel or bounded or convergence'`
- `pytest -q tests/unit_tests/flux/strategies/makerv3/test_order_safety.py -k 'cancel_rejected or venue_protection'`
- `pytest -q tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py -k 'pending_cancel or state'`

**Step 1: Write failing planner tests for unbounded churn**

Add tests that encode the current broken behavior as failures:

- widening one side of a deep ladder must not cancel the whole side in one cycle
- deep-ladder convergence with one missing top level should peel passive tail incrementally
- stale budget must remain independent from aggressive-reprice budget

Also add invariant-style tests:

- planner never returns duplicate cancel actions
- planner never returns more cancels or places than budget permits
- planner never returns more total actions than the total budget permits
- planner never cancels more aggressive levels before passive-tail excess levels when solving pure capacity problems
- ordinary widening by one step cannot generate whole-side cancellation

**Step 2: Write failing quote-cycle tests for bounded pending-cancel behavior**

Add tests that require quote cycles to stop scheduling additional repricing once a bounded cancel budget is exhausted or pending-cancel backlog is present.

Be explicit about backlog modes:

- soft throttle
- hard freeze
- blocked

**Step 3: Write failing state/export tests for cancel-storm safety**

Add tests that assert:

- `_quote_progress_payload` is safe when pending-cancel sets mutate during event handling
- terminal blocked/stopped state remains publishable under venue-protection paths

**Step 4: Run tests to verify they fail for the right reasons**

Run the verification commands above and confirm failures describe missing bounded-convergence or state-safety behavior, not unrelated fixtures.

**Step 5: Commit**

```bash
git add tests/unit_tests/flux/strategies/makerv3/test_rebalancing.py \
        tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py \
        tests/unit_tests/flux/strategies/makerv3/test_order_safety.py \
        tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py
git commit -m "test: lock MakerV3 bounded convergence behavior"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 2: Introduce Pure Bounded-Convergence Planner

**Files:**
- Modify: `systems/flux/flux/strategies/makerv3/rebalancing.py`
- Modify: `tests/unit_tests/flux/strategies/makerv3/test_rebalancing.py`

**Dependencies:** `Task 1: Reproduce Failure Mode And Lock Spec Tests`

**Write Scope:** `systems/flux/flux/strategies/makerv3/rebalancing.py`, `tests/unit_tests/flux/strategies/makerv3/test_rebalancing.py`

**Verification Commands:**
- `pytest -q tests/unit_tests/flux/strategies/makerv3/test_rebalancing.py`

**Step 1: Replace implicit broad-cancel planning with explicit bounded action planning**

Add a new pure planner surface in `rebalancing.py` that returns structured side actions with explicit ordering and budgets. Do not mix it with strategy runtime state or venue I/O.

Planner inputs should include:

- sorted active prices
- stale flags
- desired level tuples
- side
- bounded cancel/place budget inputs
- explicit backlog classification inputs

Planner outputs should include:

- ordered cancel actions
- ordered place level indices
- convergence diagnostics for telemetry

**Step 2: Encode directional convergence policy in the pure planner**

The planner must prioritize:

- excess passive tail removal
- limited aggressive-side widening cancels
- limited room creation for more aggressive missing levels
- bounded stale replacement

More specifically:

- cancel excess passive tail before peeling any in-range order
- cancel stale orders before peeling an in-range order for room
- peel at most the minimum passive room needed for aggressive missing levels in a cycle
- maintain a `keep` bucket so marginally suboptimal levels survive normal noise

It must not emit whole-side cancel sets for ordinary widening moves.

**Step 3: Keep complexity predictable**

Stay in side-local pure compute. Avoid venue objects, Redis access, dynamic cache lookups, or per-cycle hidden mutation in the planner.

**Step 4: Run the planner tests**

Run the verification command above and confirm the planner now satisfies the locked behavior from Task 1.

**Step 5: Commit**

```bash
git add systems/flux/flux/strategies/makerv3/rebalancing.py \
        tests/unit_tests/flux/strategies/makerv3/test_rebalancing.py
git commit -m "feat: add bounded convergence planner for MakerV3 ladders"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 3: Integrate Planner Into Quote Cycle With Venue Budgets

**Files:**
- Modify: `systems/flux/flux/strategies/makerv3/strategy.py`
- Modify: `systems/flux/flux/strategies/makerv3/quote_engine.py`
- Modify: `systems/flux/flux/strategies/makerv3/runtime_params.py`
- Modify: `systems/flux/flux/common/params.py`
- Modify: `tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py`
- Modify: `tests/unit_tests/flux/strategies/makerv3/test_runtime_params.py`

**Dependencies:** `Task 2: Introduce Pure Bounded-Convergence Planner`

**Write Scope:** `systems/flux/flux/strategies/makerv3/strategy.py`, `systems/flux/flux/strategies/makerv3/quote_engine.py`, `systems/flux/flux/strategies/makerv3/runtime_params.py`, `systems/flux/flux/common/params.py`, `tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py`, `tests/unit_tests/flux/strategies/makerv3/test_runtime_params.py`

**Verification Commands:**
- `pytest -q tests/unit_tests/flux/strategies/makerv3/test_runtime_params.py`
- `pytest -q tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py`

**Step 1: Add bounded-convergence runtime params**

Add explicit runtime/config params for:

- max cancels per side per cycle
- max places per side per cycle
- max total change actions per cycle
- max pending cancels per side before throttling

Keep naming simple and portable across Bybit/OKX/Bitget.

**Step 2: Replace `_rebalance_side()` full-action emission with bounded execution**

Wire the new planner into the strategy/quote cycle so that only the allowed bounded action set is executed each cycle.

**Step 3: Preserve fast-path quote-cycle behavior**

Do not add extra cache scans or repeated recomputation inside the hot path. Compute desired levels once, active orders once, planner decision once per side.

**Step 4: Ensure pending-cancel backlog uses explicit soft and hard modes**

Implement simple v1 backlog semantics:

- soft throttle when backlog count reaches the configured threshold
- hard freeze on that side when backlog or age crosses the hard threshold
- blocked state only when backlog age/size remains pathological

Do not use a blanket all-or-nothing freeze for every transient pending cancel.

**Step 5: Run the runtime and quote-engine suites**

Run the verification commands above and confirm the new planner semantics hold under quote-cycle tests.

**Step 6: Commit**

```bash
git add systems/flux/flux/strategies/makerv3/strategy.py \
        systems/flux/flux/strategies/makerv3/quote_engine.py \
        systems/flux/flux/strategies/makerv3/runtime_params.py \
        systems/flux/flux/common/params.py \
        tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py \
        tests/unit_tests/flux/strategies/makerv3/test_runtime_params.py
git commit -m "feat: bound MakerV3 ladder convergence by venue-safe budgets"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 4: Fix Cancel-Reject Reconciliation And State Export Correctness

**Files:**
- Modify: `systems/flux/flux/strategies/makerv3/strategy.py`
- Modify: `systems/flux/flux/strategies/makerv3/publisher.py`
- Modify: `tests/unit_tests/flux/strategies/makerv3/test_order_safety.py`
- Modify: `tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py`
- Modify: `tests/unit_tests/flux/api/test_app.py`

**Dependencies:** `Task 1: Reproduce Failure Mode And Lock Spec Tests`

**Write Scope:** `systems/flux/flux/strategies/makerv3/strategy.py`, `systems/flux/flux/strategies/makerv3/publisher.py`, `tests/unit_tests/flux/strategies/makerv3/test_order_safety.py`, `tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py`, `tests/unit_tests/flux/api/test_app.py`

**Verification Commands:**
- `pytest -q tests/unit_tests/flux/strategies/makerv3/test_order_safety.py`
- `pytest -q tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py -k 'pending_cancel or state'`
- `pytest -q tests/unit_tests/flux/api/test_app.py -k 'running or state'`

**Step 1: Snapshot mutable pending-cancel collections during export**

Fix state export so quote-progress and blocker payloads cannot fail when pending-cancel sets mutate during event handling.

**Step 2: Reconcile managed-order tracking on terminal-ish cancel rejects**

For venue-side reasons that indicate the order is already gone or state-mismatched, reconcile the strategy’s managed-order tracker so tracked IDs cannot accumulate indefinitely.

**Step 3: Preserve terminal blocked/stopped state visibility**

Ensure venue-protection or stop paths leave behind a durable terminal state that Flux can surface truthfully even if no further quote-state updates occur.

Make the UI/API semantics explicit:

- process alive
- strategy running
- strategy terminal-stopped
- strategy stale/unreachable

These must remain distinguishable.

**Step 4: Run correctness suites**

Run the verification commands above and confirm the Bybit cancel-storm path is observable and internally consistent.

**Step 5: Commit**

```bash
git add systems/flux/flux/strategies/makerv3/strategy.py \
        systems/flux/flux/strategies/makerv3/publisher.py \
        tests/unit_tests/flux/strategies/makerv3/test_order_safety.py \
        tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py \
        tests/unit_tests/flux/api/test_app.py
git commit -m "fix: harden MakerV3 cancel lifecycle and state export"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 5: Rollout Profiles, Telemetry, And Verification

**Files:**
- Modify: `deploy/tokenmm/strategies/plumeusdt_bybit_perp_makerv3.toml`
- Modify: `deploy/tokenmm/strategies/plumeusdt_okx_perp_makerv3.toml`
- Modify: `deploy/tokenmm/strategies/plumeusdt_bitget_perp_makerv3.toml`
- Modify: `tests/unit_tests/flux/strategies/makerv3/*`
- Modify: `tests/unit_tests/flux/api/*`
- Modify: `docs/runbooks/*` or `docs/plans/*` as needed

**Dependencies:** `Task 4: Fix Cancel-Reject Reconciliation And State Export Correctness`

**Write Scope:** `deploy/tokenmm/strategies/plumeusdt_bybit_perp_makerv3.toml`, `deploy/tokenmm/strategies/plumeusdt_okx_perp_makerv3.toml`, `deploy/tokenmm/strategies/plumeusdt_bitget_perp_makerv3.toml`, `tests/unit_tests/flux/strategies/makerv3`, `tests/unit_tests/flux/api`, `docs/runbooks`, `docs/plans`

**Verification Commands:**
- `pytest -q tests/unit_tests/flux/strategies/makerv3/test_rebalancing.py tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py tests/unit_tests/flux/strategies/makerv3/test_order_safety.py tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py tests/unit_tests/flux/strategies/makerv3/test_runtime_params.py`
- `pytest -q tests/unit_tests/flux/api/test_app.py tests/unit_tests/flux/api/test_payloads.py`

**Step 1: Set venue-fit rollout defaults**

Define initial bounded-convergence defaults for Bybit, OKX, and Bitget that preserve deep ladders while constraining burst rate. Keep the defaults explicit and reviewable in strategy TOMLs.

**Step 2: Add telemetry for convergence and budget consumption**

Expose enough quote-cycle diagnostics to answer:

- how many levels changed this cycle
- why changes were chosen
- whether budget limits were hit
- whether backlog throttling was active
- whether the strategy is healthy-but-lagging vs blocked

Keep the telemetry compact and derived from already-computed planner outputs where possible.

**Step 3: Run full targeted verification**

Run the verification commands above and confirm bounded convergence, state correctness, and API surfacing.

**Step 4: Document rollout and rollback**

Update the relevant runbook or plan notes with:

- rollout order
- metrics/log lines to watch
- rollback params
- what constitutes healthy steady-state deep-ladder behavior

**Step 5: Commit**

```bash
git add deploy/tokenmm/strategies/plumeusdt_bybit_perp_makerv3.toml \
        deploy/tokenmm/strategies/plumeusdt_okx_perp_makerv3.toml \
        deploy/tokenmm/strategies/plumeusdt_bitget_perp_makerv3.toml \
        tests/unit_tests/flux/strategies/makerv3 \
        tests/unit_tests/flux/api \
        docs/runbooks \
        docs/plans
git commit -m "chore: roll out bounded convergence for tokenmm MakerV3 ladders"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

## Verification And Review Notes

- The worktree for this plan is `/home/ubuntu/nautilus_trader/.worktrees/codex-bybit-makerv3-ladder-hardening-20260312`.
- Branch name is `codex/bybit-makerv3-ladder-hardening-20260312`.
- A direct pytest baseline from the fresh worktree currently requires local native Nautilus build artifacts. Before implementation execution, either:
  - build the worktree (`uv run --group test python build.py` or project-standard equivalent), or
  - execute tests from an environment where compiled artifacts are already present.

## Acceptance Criteria

- Deep 3-band ladders can remain live on Bybit, OKX, and Bitget without whole-side or whole-book churn during ordinary market movement.
- A single quote cycle cannot emit a venue-unsafe cancel burst.
- Venue protection remains a last-resort safety path, not a routine operational state.
- Cancel rejects and stop paths leave internal tracked-order state and exported Flux state consistent.
- Flux UI truthfully reflects stopped vs stale vs running strategy state after venue-protection events.
- In a one-step widening move on a deep side, cancels are capped by the configured aggressive cancel budget rather than whole-side replacement.
- When one more-aggressive level is missing and the side is at capacity, the planner removes at most the minimum passive tail room required under budget.
- Pending-cancel backlog above threshold prevents further repricing on that side without forcing a global strategy freeze for every transient backlog event.
- Terminal-ish cancel rejects such as `order not exists or too late to cancel` reconcile managed tracking within one event cycle.
- Telemetry distinguishes healthy bounded convergence from persistent lag and from blocked state.

## Execution Recommendation

Recommended execution mode: `superpowers:subagent-driven-development`.

Reason: this work cleanly splits into pure planner/test work, runtime/config wiring, and correctness/observability hardening, but the surfaces are still coupled enough that same-session orchestration with pinned review checkpoints is the safer choice.

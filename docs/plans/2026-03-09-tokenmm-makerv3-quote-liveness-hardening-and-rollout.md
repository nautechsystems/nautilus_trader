# TokenMM MakerV3 Quote Liveness Hardening And Rollout Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.
> **Progress:** The Progress Tracker in this document is the execution source of truth and must be updated on every state change.

**Goal:** Make MakerV3 quote liveness truthful, recoverable, and operator-visible across all TokenMM venues so a node can never show `bot_on=true` / `tradeable=true` while silently not quoting; ship the change via an OKX canary first, then roll it out fleet-wide.

**Architecture:** Treat the current OKX incident as a generic MakerV3 state-machine and observability gap, not an OKX-specific quirk. Replace the raw pending-cancel set with a venue-agnostic pending-action registry carrying age and evidence, distinguish transient cancel-in-flight from pathological stuck-cancel and no-progress states, and publish canonical blocker metadata from strategy state through API, Fluxboard, and Pulse. Keep the existing safe startup policy of `bot_on=false` on restart, add explicit quote-liveness acceptance gates before re-enable, and only auto-clear local pending state when cache and order truth both agree the order is gone.

**Tech Stack:** Flux MakerV3 strategy/runtime params/publisher/quote engine, Flux API/socketio/bridge, Nautilus cache and execution engine, Fluxboard, Pulse UI, TokenMM deploy docs/runbooks, pytest, vitest, Pulse/systemd, curl-based prod validation.

## Incident Facts This Plan Must Eliminate

1. A node can currently remain `running`, `tradeable=true`, and `blocked=false` while quoting nothing.
2. `skip_pending_cancels` lives only in the event stream; it does not raise a first-class blocker or actionable alert.
3. The current live OKX symptom appears stale-state-like: quote cycles are still running on March 9, 2026 UTC, but the last real order event was on March 8, 2026 UTC.
4. An orphaned open-order cache/index entry can coexist with no backing order record, so strategy-level state alone is not enough.
5. Restart currently clears the in-memory latch, but the operational workflow is implicit rather than encoded as a trader-facing contract.

## Production Invariants

1. `tradeable=true` must require `bot_on=true`, fresh market data, no blocking quote-liveness blocker, and recent quote progress.
2. Pending cancels are normal only inside a short grace window and only while order/quote progress continues.
3. If `bot_on=true` and the strategy cannot make forward quote progress beyond budget, it must move to an explicit blocked state and emit an alert.
4. Auto-heal must clear local pending-cancel state only when local order/cache truth shows no live order remains.
5. No new logic may special-case `plumeusdt_okx_perp_makerv3`; the implementation lives in shared MakerV3 and lower-level execution/cache surfaces.
6. Normal prod lifecycle remains Pulse-managed with restart-safe `bot_on=false` and explicit post-restart validation before re-enable.

## Progress Tracker

**Source of truth:** Update this table whenever task state changes. Do not rely on memory, chat history, or TodoWrite alone.

| Task | Status | Owner | Notes / Last Update |
| --- | --- | --- | --- |
| Overall | in_progress | main | Verified backend critical path now covers recent-vs-aged pending cancels, orphan cleanup, quote progress/blocker metadata, actionable alert emission, API tradeability honesty, and TokenMM alert routing; lower-level cache cleanup, full blocker registry, and rollout/runbook work remain |
| Task 1: Build The Quote-Liveness Regression Matrix | in_progress | main | Critical MakerV3/API/bridge regression slice is green across `test_quote_engine.py`, `test_observability_and_exports.py`, `test_order_safety.py`, `test_runtime_params.py`, `tests/unit_tests/flux/api/test_payloads.py`, `test_tokenmm_run_bridge.py`, and `test_tokenmm_run_api.py`; lower-level execution/cache matrix still pending |
| Task 2: Replace Raw Pending-Cancel Tracking With A Canonical Blocker Registry | in_progress | main | Shared runtime budgets landed (`pending_cancel_grace_ms`, `pending_cancel_block_after_ms`, `quote_liveness_stall_after_ms`, `quote_liveness_recover_after_ms`) plus first-seen timestamp bookkeeping; full per-order blocker registry replacement still pending |
| Task 3: Promote Quote Liveness To A First-Class Strategy State Contract | in_progress | main | `blocked_pending_cancel`, recent-vs-stuck pending-cancel behavior, quote progress timestamps/ages, and orphan cleanup are implemented and verified; explicit `blocked_quote_liveness` stall state still pending |
| Task 4: Harden Nautilus Order Cache And Reconciliation Cleanup | not_started | unassigned | Plan created |
| Task 5: Emit Actionable Alerts And Honest API Tradeability | in_progress | main | `quote_liveness_blocked` actionable alert now emits on `blocked_pending_cancel`, signals mark quote blockers not tradeable, and TokenMM bridge alert routing is verified; broader liveness-stall alerting still pending |
| Task 6: Surface Blockers In Fluxboard And Pulse | not_started | unassigned | Plan created |
| Task 7: Encode The Trader Runbook, Audit Tooling, And Deploy Contract | not_started | unassigned | Plan created |
| Task 8: Execute OKX Canary, Then Fleet Rollout | not_started | unassigned | Plan created |

---

### Task 1: Build The Quote-Liveness Regression Matrix

**Files:**
- Modify: `tests/unit_tests/flux/strategies/makerv3/test_order_safety.py`
- Modify: `tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py`
- Modify: `tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py`
- Modify: `tests/unit_tests/flux/strategies/makerv3/test_runtime_params.py`
- Modify: `tests/unit_tests/flux/strategies/makerv3/test_managed_orders.py`
- Modify: `tests/unit_tests/live/test_execution_engine.py`
- Modify: `tests/unit_tests/cache/test_execution.py`
- Modify: `tests/unit_tests/examples/strategies/test_tokenmm_run_bridge.py`
- Modify: `tests/unit_tests/examples/strategies/test_tokenmm_run_api.py`
- Modify: `tests/unit_tests/flux/api/test_socketio_tokenmm.py`

**Step 1: Add the failing cross-venue regression cases**

At minimum add red tests for:

```python
def test_refresh_quotes_blocks_when_pending_cancel_is_old_and_no_quote_progress():
    ...
    assert state == "blocked_pending_cancel"
```

```python
def test_refresh_quotes_clears_orphaned_pending_cancel_when_cache_confirms_no_live_order():
    ...
    assert strategy._pending_cancel_client_order_ids == set()
```

```python
def test_signal_payload_marks_stuck_pending_cancel_strategy_not_tradeable():
    ...
    assert payload["tradeable"] is False
    assert payload["blocked"] is True
```

```python
def test_alert_bridge_routes_pending_cancel_stuck_alert_to_alert_handler():
    ...
    assert FULL_TO_SUFFIX_TOPICS[TOPIC_ALERT] == "alert"
```

```python
def test_execution_cache_clears_orders_open_index_on_terminal_state_mismatch():
    ...
    assert not cache.is_order_open(order.client_order_id)
```

**Step 2: Run the failing backend regression slice**

Run:

```bash
pytest \
  tests/unit_tests/flux/strategies/makerv3/test_order_safety.py \
  tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py \
  tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py \
  tests/unit_tests/flux/strategies/makerv3/test_runtime_params.py \
  tests/unit_tests/flux/strategies/makerv3/test_managed_orders.py \
  tests/unit_tests/live/test_execution_engine.py \
  tests/unit_tests/cache/test_execution.py \
  tests/unit_tests/examples/strategies/test_tokenmm_run_bridge.py \
  tests/unit_tests/examples/strategies/test_tokenmm_run_api.py \
  tests/unit_tests/flux/api/test_socketio_tokenmm.py -v
```

Expected: FAIL only on the new quote-liveness, alerting, and cache-cleanup gaps.

**Step 3: Commit**

```bash
git add \
  tests/unit_tests/flux/strategies/makerv3/test_order_safety.py \
  tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py \
  tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py \
  tests/unit_tests/flux/strategies/makerv3/test_runtime_params.py \
  tests/unit_tests/flux/strategies/makerv3/test_managed_orders.py \
  tests/unit_tests/live/test_execution_engine.py \
  tests/unit_tests/cache/test_execution.py \
  tests/unit_tests/examples/strategies/test_tokenmm_run_bridge.py \
  tests/unit_tests/examples/strategies/test_tokenmm_run_api.py \
  tests/unit_tests/flux/api/test_socketio_tokenmm.py
git commit -m "test(tokenmm): add quote liveness and stale cancel regression matrix"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 2: Replace Raw Pending-Cancel Tracking With A Canonical Blocker Registry

**Files:**
- Modify: `systems/flux/flux/common/params.py`
- Modify: `systems/flux/flux/strategies/makerv3/runtime_params.py`
- Modify: `systems/flux/flux/strategies/makerv3/strategy.py`
- Modify: `systems/flux/flux/strategies/makerv3/managed_orders.py`
- Test: `tests/unit_tests/flux/strategies/makerv3/test_order_safety.py`
- Test: `tests/unit_tests/flux/strategies/makerv3/test_runtime_params.py`
- Test: `tests/unit_tests/flux/strategies/makerv3/test_managed_orders.py`

**Step 1: Add venue-agnostic runtime budgets**

Add shared MakerV3 runtime params for:

- `pending_cancel_grace_ms`
- `pending_cancel_block_after_ms`
- `quote_liveness_stall_after_ms`
- `quote_liveness_recover_after_ms`

Keep the defaults generic across all MakerV3 venues. Do not add any `strategy_id` or venue-specific branching here.

**Step 2: Replace the raw set with a richer pending-action registry**

Track per-client-order pending-cancel records with:

- `client_order_id`
- `venue_order_id`
- `first_seen_ns`
- `last_progress_ns`
- `source` such as `rebalance`, `startup_cleanup`, or `operator_stop`
- optional local order/cache evidence

Add helper methods that answer:

- is this cancel still transient?
- is this cancel orphaned locally?
- is this cancel ambiguous and therefore blocking?

**Step 3: Preserve restart-safe semantics**

Keep `on_start` clearing local in-memory blockers, but make the startup path immediately rebuild only from live cache truth rather than from stale local sets. Fresh restarts must remain `bot_on=false` until the operator re-enables quoting.

**Step 4: Run the Task 2 slice**

Run:

```bash
pytest \
  tests/unit_tests/flux/strategies/makerv3/test_order_safety.py \
  tests/unit_tests/flux/strategies/makerv3/test_runtime_params.py \
  tests/unit_tests/flux/strategies/makerv3/test_managed_orders.py -v
```

Expected: PASS with the new runtime params and blocker bookkeeping contract.

**Step 5: Commit**

```bash
git add \
  systems/flux/flux/common/params.py \
  systems/flux/flux/strategies/makerv3/runtime_params.py \
  systems/flux/flux/strategies/makerv3/strategy.py \
  systems/flux/flux/strategies/makerv3/managed_orders.py \
  tests/unit_tests/flux/strategies/makerv3/test_order_safety.py \
  tests/unit_tests/flux/strategies/makerv3/test_runtime_params.py \
  tests/unit_tests/flux/strategies/makerv3/test_managed_orders.py
git commit -m "feat(makerv3): replace raw pending cancel tracking with blocker registry"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 3: Promote Quote Liveness To A First-Class Strategy State Contract

**Files:**
- Modify: `systems/flux/flux/strategies/makerv3/constants.py`
- Modify: `systems/flux/flux/strategies/makerv3/quote_engine.py`
- Modify: `systems/flux/flux/strategies/makerv3/publisher.py`
- Modify: `systems/flux/flux/strategies/makerv3/strategy.py`
- Test: `tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py`
- Test: `tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py`

**Step 1: Define canonical blocker states and reason codes**

Add explicit state and reason-code coverage for:

- `blocked_pending_cancel`
- `blocked_quote_liveness`
- `pending_cancel_stuck`
- `quote_liveness_stalled`
- `orphaned_pending_cancel_cleared`

Fresh cancels inside the grace window may still emit `skip_pending_cancels`, but aged or no-progress cancels must escalate to a blocked state.

**Step 2: Publish quote progress and blocker metadata**

Expose canonical state fields such as:

- `quote_blockers`
- `quote_progress.last_completed_quote_ts_ms`
- `quote_progress.last_order_event_ts_ms`
- `quote_progress.pending_cancel_count`
- `quote_progress.oldest_pending_cancel_age_ms`
- `quote_progress.orphaned_pending_cancel_count`

The state payload should make it impossible for downstream consumers to infer health from `state="running"` alone.

**Step 3: Add safe auto-heal for proven orphans**

If a pending cancel ages past the grace window and local cache shows no corresponding open or inflight order, clear it once, publish `orphaned_pending_cancel_cleared`, and continue. If the strategy still has no quote progress after that, escalate to `blocked_quote_liveness`.

**Step 4: Run the Task 3 slice**

Run:

```bash
pytest \
  tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py \
  tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py -v
```

Expected: PASS with explicit blocker state, progress metadata, and event coverage.

**Step 5: Commit**

```bash
git add \
  systems/flux/flux/strategies/makerv3/constants.py \
  systems/flux/flux/strategies/makerv3/quote_engine.py \
  systems/flux/flux/strategies/makerv3/publisher.py \
  systems/flux/flux/strategies/makerv3/strategy.py \
  tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py \
  tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py
git commit -m "feat(makerv3): make quote liveness a first-class blocker contract"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 4: Harden Nautilus Order Cache And Reconciliation Cleanup

**Files:**
- Modify: `nautilus_trader/cache/cache.pyx`
- Modify: `nautilus_trader/cache/database.pyx`
- Modify: `nautilus_trader/execution/engine.pyx`
- Test: `tests/unit_tests/cache/test_execution.py`
- Test: `tests/unit_tests/live/test_execution_engine.py`

**Step 1: Add failing cache/index cleanup coverage**

Cover:

- terminal order events remove stale `orders_open` and pending-cancel index membership even when the order payload is partially missing
- cancel-rejected state-mismatch cleanup leaves no orphaned open-order index
- startup or reconciliation load does not preserve open-order IDs whose order record is missing

**Step 2: Implement cleanup at the lowest safe layer**

Make cache/execution cleanup idempotent:

- terminal events always purge `orders_open`
- pending-cancel index cleanup happens on all terminal or state-mismatch paths
- missing order records do not block index cleanup
- emit one warning/alertable signal when orphaned cache state is detected and repaired

**Step 3: Run the Task 4 slice**

Run:

```bash
pytest \
  tests/unit_tests/cache/test_execution.py \
  tests/unit_tests/live/test_execution_engine.py -v
```

Expected: PASS with deterministic cleanup of stale cache/index state.

**Step 4: Commit**

```bash
git add \
  nautilus_trader/cache/cache.pyx \
  nautilus_trader/cache/database.pyx \
  nautilus_trader/execution/engine.pyx \
  tests/unit_tests/cache/test_execution.py \
  tests/unit_tests/live/test_execution_engine.py
git commit -m "fix(execution): purge orphaned open-order and pending-cancel cache state"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 5: Emit Actionable Alerts And Honest API Tradeability

**Files:**
- Modify: `systems/flux/flux/strategies/makerv3/constants.py`
- Modify: `systems/flux/flux/strategies/makerv3/publisher.py`
- Modify: `systems/flux/flux/api/_payloads_signals.py`
- Modify: `systems/flux/flux/api/socketio.py`
- Test: `tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py`
- Test: `tests/unit_tests/examples/strategies/test_tokenmm_run_bridge.py`
- Test: `tests/unit_tests/examples/strategies/test_tokenmm_run_api.py`
- Test: `tests/unit_tests/flux/api/test_socketio_tokenmm.py`

**Step 1: Add new actionable alert keys**

Add end-to-end alert coverage for:

- `pending_cancel_stuck`
- `quote_liveness_stalled`
- `order_state_orphaned`

Each alert should include enough operator context to act:

- strategy id
- blocker reason
- oldest age ms
- pending cancel count
- recommended operator action such as `leave_bot_off`, `restart_node`, or `inspect_cache`

**Step 2: Make `tradeable` and `blocked` reflect blocker truth**

Update signal payload construction so `tradeable` is not derived from `bot_on` plus `state.startswith("blocked_")` alone. If blocker metadata says quote progress is stalled, the API must return `tradeable=false` and `blocked=true` even if state naming drifts.

**Step 3: Keep socket payloads and alert previews aligned**

Socket and alert preview surfaces must update when:

- a blocker is added or cleared
- a quote-liveness alert fires or recovers
- a strategy moves from transient cancel-in-flight to blocked

**Step 4: Run the Task 5 slice**

Run:

```bash
pytest \
  tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py \
  tests/unit_tests/examples/strategies/test_tokenmm_run_bridge.py \
  tests/unit_tests/examples/strategies/test_tokenmm_run_api.py \
  tests/unit_tests/flux/api/test_socketio_tokenmm.py -v
```

Expected: PASS with new alert keys, honest `tradeable` semantics, and socket parity.

**Step 5: Commit**

```bash
git add \
  systems/flux/flux/strategies/makerv3/constants.py \
  systems/flux/flux/strategies/makerv3/publisher.py \
  systems/flux/flux/api/_payloads_signals.py \
  systems/flux/flux/api/socketio.py \
  tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py \
  tests/unit_tests/examples/strategies/test_tokenmm_run_bridge.py \
  tests/unit_tests/examples/strategies/test_tokenmm_run_api.py \
  tests/unit_tests/flux/api/test_socketio_tokenmm.py
git commit -m "feat(tokenmm): publish quote liveness alerts and honest tradeability"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 6: Surface Blockers In Fluxboard And Pulse

**Files:**
- Modify: `fluxboard/utils/signalRunState.ts`
- Modify: `fluxboard/components/domain/signal/SignalTable.tsx`
- Modify: `fluxboard/components/domain/alerts/AlertsTable.tsx`
- Modify: `pulse-ui/src/App.tsx`
- Modify: `pulse-ui/src/components/JobRow.tsx`
- Modify if needed: `pulse-ui/src/components/StatusPill.tsx`
- Test: `fluxboard/__tests__/panels/signal.test.tsx`
- Test: `fluxboard/__tests__/panels/alerts.test.tsx`
- Test: `fluxboard/tests/signal/SignalTable.audit.test.tsx`
- Test: `pulse-ui/src/App.test.tsx`

**Step 1: Make the trader UI show the true failure mode**

Signal rows should show:

- blocker state such as `blocked_pending_cancel`
- pending cancel count and oldest age
- last quote progress age
- whether the node is safe to re-enable or requires restart/investigation

Do not make the user click into raw alerts to discover that `bot_on=true` still means “not actually quoting”.

**Step 2: Make alerts operator-friendly**

Alerts should render the new action hints and blocker context directly. The table should make it obvious whether the correct next action is:

- leave the node bot-off
- restart the node through Pulse
- wait for transient recovery
- investigate lower-level execution/cache state

**Step 3: Make Pulse distinguish healthy from merely running**

Pulse should keep the process-up view, but also surface node-level blocker summaries so an operator can see “process up, quoting blocked” without cross-referencing a second screen.

**Step 4: Run the Task 6 frontend slice**

Run:

```bash
pnpm --dir fluxboard exec vitest run \
  __tests__/panels/signal.test.tsx \
  __tests__/panels/alerts.test.tsx \
  tests/signal/SignalTable.audit.test.tsx

pnpm --dir pulse-ui exec vitest run src/App.test.tsx
```

Expected: PASS with blocker-aware signal, alerts, and Pulse operator views.

**Step 5: Commit**

```bash
git add \
  fluxboard/utils/signalRunState.ts \
  fluxboard/components/domain/signal/SignalTable.tsx \
  fluxboard/components/domain/alerts/AlertsTable.tsx \
  pulse-ui/src/App.tsx \
  pulse-ui/src/components/JobRow.tsx \
  pulse-ui/src/components/StatusPill.tsx \
  fluxboard/__tests__/panels/signal.test.tsx \
  fluxboard/__tests__/panels/alerts.test.tsx \
  fluxboard/tests/signal/SignalTable.audit.test.tsx \
  pulse-ui/src/App.test.tsx
git commit -m "feat(ui): surface quote blockers in fluxboard and pulse"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 7: Encode The Trader Runbook, Audit Tooling, And Deploy Contract

**Files:**
- Create: `docs/architecture/tokenmm-quote-liveness.md`
- Modify: `deploy/tokenmm/README.md`
- Modify: `deploy/tokenmm/strategies/README.md`
- Modify: `docs/runbooks/tokenmm-risk-validation.md`
- Modify: `fluxboard/docs/tokenmm_contract.md`
- Modify: `fluxboard/docs/tokenmm_socket_contract.md`
- Modify: `scripts/ops/tokenmm_risk_audit.py`
- Test: `tests/unit_tests/examples/strategies/test_tokenmm_risk_validation_contract.py`
- Test: `tests/unit_tests/examples/strategies/test_tokenmm_stack_contract.py`

**Step 1: Document the new contract**

Write down the operator truths explicitly:

- `bot_on=false` after restart is intentional and required
- `tradeable=true` means quote-liveness healthy, not just process-up
- `blocked_pending_cancel` and `blocked_quote_liveness` are stop states
- transient pending cancels are normal; aged blockers are not
- OKX is the first canary, but the contract is MakerV3-wide

**Step 2: Extend the audit script**

Teach `scripts/ops/tokenmm_risk_audit.py` to fail on:

- blocked quote-liveness states
- aged pending cancel blockers
- stale alert surfaces for an active blocker
- strategy rows that claim `tradeable=true` while blocker metadata says otherwise

**Step 3: Encode the post-restart trader checklist**

The runbook must say:

1. restart from Pulse for normal operations
2. confirm jobs are up
3. confirm the node is either healthy or explicitly blocked with reason
4. confirm `signals`, `alerts`, and `Pulse` agree
5. only then re-enable quoting

**Step 4: Run the Task 7 contract slice**

Run:

```bash
pytest \
  tests/unit_tests/examples/strategies/test_tokenmm_risk_validation_contract.py \
  tests/unit_tests/examples/strategies/test_tokenmm_stack_contract.py -v

python3 scripts/ops/tokenmm_risk_audit.py --help
```

Expected: PASS and the audit script exposes the new quote-liveness checks.

**Step 5: Commit**

```bash
git add \
  docs/architecture/tokenmm-quote-liveness.md \
  deploy/tokenmm/README.md \
  deploy/tokenmm/strategies/README.md \
  docs/runbooks/tokenmm-risk-validation.md \
  fluxboard/docs/tokenmm_contract.md \
  fluxboard/docs/tokenmm_socket_contract.md \
  scripts/ops/tokenmm_risk_audit.py \
  tests/unit_tests/examples/strategies/test_tokenmm_risk_validation_contract.py \
  tests/unit_tests/examples/strategies/test_tokenmm_stack_contract.py
git commit -m "docs(tokenmm): encode quote liveness ops contract and audit tooling"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 8: Execute OKX Canary, Then Fleet Rollout

**Files:**
- Reference: `docs/runbooks/tokenmm-risk-validation.md`
- Reference: `deploy/tokenmm/README.md`
- Reference: `docs/architecture/tokenmm-quote-liveness.md`

**Step 1: Run the final pre-prod verification bundle**

Run:

```bash
pytest \
  tests/unit_tests/flux/strategies/makerv3/test_order_safety.py \
  tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py \
  tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py \
  tests/unit_tests/flux/strategies/makerv3/test_runtime_params.py \
  tests/unit_tests/flux/strategies/makerv3/test_managed_orders.py \
  tests/unit_tests/live/test_execution_engine.py \
  tests/unit_tests/cache/test_execution.py \
  tests/unit_tests/examples/strategies/test_tokenmm_run_bridge.py \
  tests/unit_tests/examples/strategies/test_tokenmm_run_api.py \
  tests/unit_tests/flux/api/test_socketio_tokenmm.py \
  tests/unit_tests/examples/strategies/test_tokenmm_risk_validation_contract.py \
  tests/unit_tests/examples/strategies/test_tokenmm_stack_contract.py -v

pnpm --dir fluxboard exec vitest run \
  __tests__/panels/signal.test.tsx \
  __tests__/panels/alerts.test.tsx \
  tests/signal/SignalTable.audit.test.tsx

pnpm --dir pulse-ui exec vitest run src/App.test.tsx
```

Expected: all targeted suites green.

**Step 2: Run the OKX canary restart in the standard operator flow**

Use Pulse for the normal restart path, then verify with:

```bash
curl -fsS 'http://127.0.0.1:5022/api/pulse/jobs'
curl -fsS 'http://127.0.0.1:5022/api/v1/signals?profile=tokenmm'
curl -fsS 'http://127.0.0.1:5022/api/v1/alerts?strategy=plumeusdt_okx_perp_makerv3&profile=tokenmm'
python3 scripts/ops/tokenmm_risk_audit.py --base-url http://127.0.0.1:5022
```

Expected before re-enable:

- process is up in Pulse
- node starts `bot_on=false`
- no silent `tradeable=true` / zero-progress state exists
- if a blocker exists, it is explicit in both signals and alerts

**Step 3: Re-enable OKX only after acceptance gates**

OKX canary go/no-go:

- at least one recent completed quote cycle is visible
- pending cancel blockers are zero or clearly transient
- alerts are quiet except for acknowledged test or transient noise
- Fluxboard signal row and Pulse agree on status

If any blocker is ambiguous, leave `bot_on=false` and do not widen rollout.

**Step 4: Expand to one additional perp and one additional spot venue**

Use one more perp and one more spot strategy as proving grounds before all-node rollout. This catches venue-type differences without turning the first OKX pass into a fleet assumption.

**Step 5: Roll out to the remaining TokenMM MakerV3 fleet**

Only widen after:

- 30 to 60 minutes of clean OKX canary behavior
- one additional perp canary passes
- one additional spot canary passes
- no false-positive blocker storm appears in Fluxboard alerts

**Step 6: Rollback**

Rollback path:

1. set the affected node `bot_on=false`
2. restart through Pulse if state cleanup is required
3. revert the deploy to the previous known-good revision
4. keep the node disabled until `signals`, `alerts`, and Pulse are consistent again

**Step 7: Commit**

No code commit for the rollout itself. If the canary requires threshold tuning, land that as a separate follow-up commit rather than mutating the shipped contract silently.

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

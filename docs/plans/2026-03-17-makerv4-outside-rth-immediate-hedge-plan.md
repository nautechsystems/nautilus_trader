# Makerv4 Outside-RTH Immediate Hedge Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.
> **Progress:** The Progress Tracker in this document is the execution source of truth and must be updated on every state change.

**Goal:** Make MakerV4 take-take hedging remain an immediate IBKR hedge attempt outside regular US equity hours instead of silently downgrading into a resting overnight stock order.

**Architecture:** Keep one hedge intent for take-take across sessions: aggressive immediate hedge with the existing through-touch limit construction. Session-aware policy should only control route and overnight tags, not switch time-in-force from immediate to resting. If outside-RTH immediate hedging cannot be expressed or accepted, fail closed and surface the reason rather than resting passively.

**Tech Stack:** Python 3, Nautilus Trader live strategy code, IBKR adapter integration, pytest unit tests.

## Progress Tracker

**Source of truth:** Update this table whenever task state changes. Do not rely on memory, chat history, or TodoWrite alone.

| Task | Status | Owner | Notes / Last Update |
| --- | --- | --- | --- |
| Overall | in_progress | controller | Execution started via subagent-driven-development |
| Task 1: Lock Immediate Hedge Policy Contract | in_progress | implementer | Commit `bd26fb46bf` landed; `uv run --group test python -m pytest tests/unit_tests/flux/strategies/shared/test_ibkr_order_policy.py -q` passed (`3 passed`); implementer follow-up needed for `ruff` import ordering |
| Task 2: Update Makerv4 Strategy Expectations | in_progress | implementer | Uncommitted overnight strategy test expectation changes detected in working tree; controller reconciling before review |
| Task 3: Implement Outside-RTH Immediate Hedge Policy | not_started | unassigned | Plan created |
| Task 4: Document and Verify Operational Behavior | not_started | unassigned | Plan created |

---

### Task 1: Lock Immediate Hedge Policy Contract

**Files:**
- Modify: `systems/flux/flux/strategies/shared/ibkr_order_policy.py`
- Test: `tests/unit_tests/flux/strategies/shared/test_ibkr_order_policy.py`

**Step 1: Write the failing policy test for outside-RTH immediate hedging**

Update `tests/unit_tests/flux/strategies/shared/test_ibkr_order_policy.py` so the outside-regular-session case expects:
- immediate `IOC` hedge attempt
- `outside_rth=True`
- overnight permission/tagging preserved
- no passive `DAY` downgrade
- no `cancel_after_ms` budget on an IOC order

Also keep a separate test for regular-session behavior so both session branches remain covered.

**Step 2: Run the focused shared-policy tests and confirm failure**

Run: `pytest tests/unit_tests/flux/strategies/shared/test_ibkr_order_policy.py -q`
Expected: FAIL because current outside-RTH policy returns a resting `DAY` order with `cancel_after_ms=5000`.

**Step 3: Implement the minimal policy change**

Update `systems/flux/flux/strategies/shared/ibkr_order_policy.py` so outside-RTH hedging for MakerV4 returns an immediate policy:
- keep route selection logic minimal for this change
- preserve outside-hours tags/permissions
- do not encode a passive timeout-driven resting mode in the policy builder

Do not bundle route redesign into this task. This task is only about stopping the TIF downgrade.

**Step 4: Re-run the shared-policy tests**

Run: `pytest tests/unit_tests/flux/strategies/shared/test_ibkr_order_policy.py -q`
Expected: PASS.

**Step 5: Commit**

```bash
git add systems/flux/flux/strategies/shared/ibkr_order_policy.py tests/unit_tests/flux/strategies/shared/test_ibkr_order_policy.py
git commit -m "fix: keep outside-rth makerv4 hedges immediate"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 2: Update Makerv4 Strategy Expectations

**Files:**
- Modify: `tests/unit_tests/flux/strategies/makerv4/test_strategy.py`

**Step 1: Write failing strategy tests for outside-RTH behavior**

Update the existing outside-RTH tests in `tests/unit_tests/flux/strategies/makerv4/test_strategy.py` so they describe the desired contract:
- the outside-RTH hedge policy test should expect `IOC`, not `DAY`
- pending hedge metadata should no longer advertise a passive overnight cancel budget for this path
- take-take hedge construction outside RTH should still create an immediate IBKR hedge order after the HL fill callback

Prefer updating the current overnight tests instead of adding duplicate coverage unless a new scenario is genuinely distinct.

**Step 2: Run the focused Makerv4 strategy tests and confirm failure**

Run: `pytest tests/unit_tests/flux/strategies/makerv4/test_strategy.py -q`
Expected: FAIL in the outside-RTH hedge policy assertions.

**Step 3: Adjust strategy-facing expectations only as required**

Keep the strategy-level assertion surface aligned with the new policy:
- outside-RTH hedge orders stay immediate
- no hidden resting-order metadata remains in state snapshots for this path
- take-take tests continue to assert hedge submission occurs only after the HL fill callback

This task should not add new behavior beyond what Task 1 established.

**Step 4: Re-run the focused Makerv4 strategy tests**

Run: `pytest tests/unit_tests/flux/strategies/makerv4/test_strategy.py -q`
Expected: PASS.

**Step 5: Commit**

```bash
git add tests/unit_tests/flux/strategies/makerv4/test_strategy.py
git commit -m "test: align makerv4 outside-rth hedge expectations"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 3: Implement Outside-RTH Immediate Hedge Policy

**Files:**
- Modify: `systems/flux/flux/strategies/shared/ibkr_order_policy.py`
- Modify: `systems/flux/flux/strategies/makerv4/strategy.py`
- Test: `tests/unit_tests/flux/strategies/shared/test_ibkr_order_policy.py`
- Test: `tests/unit_tests/flux/strategies/makerv4/test_strategy.py`

**Step 1: Write or refine the smallest failing integration-style assertion**

If Task 2 did not already cover it precisely, add one focused Makerv4 test that proves:
- an outside-RTH maker fill produces a hedge intent with immediate TIF
- the resulting submitted IBKR order uses the immediate TIF enum
- outside-hours tags remain attached

Use the existing strategy stubs and `_OVERNIGHT_TS_MS` / `_OVERNIGHT_TS_NS` helpers already present in `tests/unit_tests/flux/strategies/makerv4/test_strategy.py`.

**Step 2: Run the single targeted test and confirm failure**

Run a narrow command for the new or updated test, for example:
`pytest tests/unit_tests/flux/strategies/makerv4/test_strategy.py::test_makerv4_overnight_hedge_policy_uses_immediate_ioc_with_overnight_tags -q`

Expected: FAIL before implementation is complete.

**Step 3: Make the minimal production changes**

Update production code so the strategy uses the new outside-RTH immediate hedge policy end-to-end:
- `systems/flux/flux/strategies/shared/ibkr_order_policy.py`
  ensure outside-RTH returns immediate TIF
- `systems/flux/flux/strategies/makerv4/strategy.py`
  ensure policy/state payloads no longer imply passive overnight resting behavior for this path

Do not redesign quote validation, telemetry, or route-selection semantics in this task. Preserve fail-closed behavior on stale or invalid quotes.

**Step 4: Run targeted tests**

Run:
- `pytest tests/unit_tests/flux/strategies/shared/test_ibkr_order_policy.py -q`
- `pytest tests/unit_tests/flux/strategies/makerv4/test_strategy.py -q`

Expected: PASS.

**Step 5: Run broader Makerv4 regression coverage**

Run:
- `pytest tests/unit_tests/flux/strategies/makerv4/test_pricing.py -q`
- `pytest tests/unit_tests/flux/strategies/makerv4/test_publisher_contract.py -q`
- `pytest tests/unit_tests/flux/strategies/makerv4/test_runtime_params.py -q`

Expected: PASS with no regressions in quote validation, runtime params, or payload contract.

**Step 6: Commit**

```bash
git add systems/flux/flux/strategies/shared/ibkr_order_policy.py systems/flux/flux/strategies/makerv4/strategy.py tests/unit_tests/flux/strategies/shared/test_ibkr_order_policy.py tests/unit_tests/flux/strategies/makerv4/test_strategy.py
git commit -m "fix: keep outside-rth take-take hedges immediate"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 4: Document and Verify Operational Behavior

**Files:**
- Modify: `deploy/equities/README.md`
- Modify: `docs/plans/2026-03-16-makerv4-take-take-and-overnight-hedge.md`

**Step 1: Update operator-facing docs**

Document the new contract clearly:
- take-take hedges remain immediate outside RTH
- outside-RTH quote validity still gates submission
- stale or invalid IBKR quotes fail closed instead of downgrading into passive overnight resting hedges

Update both the operator README and the prior overnight hedge design note so the design history matches the shipped behavior.

**Step 2: Run documentation sanity checks**

Run:
- `rg -n "DAY|resting|overnight" deploy/equities/README.md docs/plans/2026-03-16-makerv4-take-take-and-overnight-hedge.md`

Expected: Any remaining mentions of passive overnight stock hedges are either removed or explicitly marked historical/superseded.

**Step 3: Run final verification commands**

Run:
- `pytest tests/unit_tests/flux/strategies/shared/test_ibkr_order_policy.py -q`
- `pytest tests/unit_tests/flux/strategies/makerv4/test_strategy.py -q`
- `pytest tests/unit_tests/flux/strategies/makerv4/test_pricing.py -q`
- `pytest tests/unit_tests/flux/strategies/makerv4/test_publisher_contract.py -q`
- `pytest tests/unit_tests/flux/strategies/makerv4/test_runtime_params.py -q`

Expected: PASS.

**Step 4: Commit**

```bash
git add deploy/equities/README.md docs/plans/2026-03-16-makerv4-take-take-and-overnight-hedge.md
git commit -m "docs: clarify immediate outside-rth hedge behavior"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

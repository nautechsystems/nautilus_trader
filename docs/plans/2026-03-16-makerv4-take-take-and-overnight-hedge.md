# MakerV4 Take-Take And Overnight Hedge Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.
> **Progress:** The Progress Tracker in this document is the execution source of truth and must be updated on every state change.

**Goal:** Finish the remaining MakerV4 production work needed for live HL-vs-IBKR basis trading by making IBKR hedge submission session-aware and fee-aware, then adding an explicit `take_take` execution mode that aggressively takes Hyperliquid first and hedges on IBKR after fill confirmation.

**Architecture:** Keep MakerV4 as one strategy family with explicit execution modes rather than splitting into multiple families. The default canary path remains one-per-side maker quoting on Hyperliquid with immediate IBKR hedge submission. `take_take` is a separate decision path inside MakerV4, not a hidden branch inside the maker quote loop. Overnight IBKR behavior must be implemented through a shared policy layer that selects route, time-in-force, tags, and cancel behavior from the session context instead of scattering venue/session rules inside strategy code.

**Tech Stack:** Python strategy/runtime code, Nautilus Trader strategy order APIs, Flux runners/API/profile contracts, Interactive Brokers adapter tags and order transforms, Hyperliquid venue adapter, Redis-backed params, pytest, official IBKR routing/commission docs, deploy TOMLs, systemd/Pulse services.

## Current Review Findings

1. MakerV4 is locally close to a live canary, but Task 5 in the current cutover plan is still blocked on Hyperliquid funded-account request headroom.
2. Live IBKR overnight execution is now proven through the Nautilus path, but the valid production shape is narrower than the original hedge design:
   - `SMART + includeOvernight=true` works.
   - direct `OVERNIGHT` route is not the default production path.
   - `IOC` is invalid on the overnight SMART stock route.
   - overnight-capable stock hedges therefore need a session-aware order policy, not a hardcoded IOC assumption.
3. The current Makerv4 hedge path still assumes immediate IOC semantics in the core design, so it must be adjusted before an overnight live canary.
4. For economics, the current live smoke fill returned `1.00 USD` commission on `BUY 1 GOOGL`, which is consistent with IBKR Fixed pricing or fixed-like treatment for the current path/account. For production basis trading we should target `IBKR Pro Tiered` and avoid fee-sensitive directed-routing assumptions.
5. `take_take` should stay in MakerV4 as an explicit mode:
   - it aggresses Hyperliquid first,
   - submits the IBKR hedge only after HL fill confirmation,
   - ignores residual hedge handling for this wave,
   - fails closed on partial/missed hedge handling rather than trying to recover automatically.
6. Fluxboard Makerv4-specific UX polish is intentionally out of scope for this plan except where backend/state exports are required for live testing and operator visibility.

## External Constraints

- IBKR official docs indicate overnight-capable US stock API orders use `includeOvernight=True`, with `SMART` for combined regular/overnight access or `OVERNIGHT` for overnight-only routing.
- Live testing in this worktree already established that `SMART + includeOvernight=true + DAY` fills, while `SMART + includeOvernight=true + IOC` is rejected as invalid for the order/security combination.
- IBKR official pricing docs indicate directed API orders do not get Tiered treatment; the production target for basis hedging should be `SMART` plus the account on `IBKR Pro Tiered`.

## Progress Tracker

**Source of truth:** Update this table whenever task state changes. Do not rely on memory, chat history, or TodoWrite alone.

| Task | Status | Owner | Notes / Last Update |
| --- | --- | --- | --- |
| Overall | blocked | main | `2026-03-16 10:15 UTC` Tasks 2-6 are closed. Task 7 is operationally blocked: equities readiness is green, but the funded Hyperliquid account still has `nRequestsSurplus=0`, so a live MakerV4 canary cannot start safely. Deferred hardening notes remain maker-side quote-age gating, operational enforcement of overnight `cancel_after_ms`, and partial-maker-fill residual handling below the hedgeable share increment in `take_take`. |
| Task 1: Lock Overnight IBKR Hedge Policy And Fee Contract In Tests And Docs | completed | main | `2026-03-16 08:09 UTC` fee-plan/runtime contract, overnight hedge docs, and focused overnight tests are green. Verification: `tests/integration_tests/adapters/interactive_brokers/test_execution_order_transform.py tests/unit_tests/flux/strategies/makerv4/test_strategy.py tests/unit_tests/flux/strategies/makerv4/test_runtime_params.py -k 'overnight or include_overnight or hedge_policy or fee_plan'` -> `6 passed, 67 deselected`. |
| Task 2: Implement Shared Session-Aware IBKR Hedge Order Policy | completed | main | `2026-03-16 09:08 UTC` shared policy seam, overnight route normalization, and broad Makerv4 regression cleanup are green. Verification: `tests/unit_tests/flux/strategies/makerv4/test_strategy.py` -> `41 passed`; `tests/unit_tests/flux/strategies/makerv4` -> `82 passed`; `git diff --check` clean. |
| Task 3: Rewire MakerV4 Hedge Submission For Session-Aware Overnight Behavior | completed | main | `2026-03-16 09:08 UTC` session-aware hedge submission, pending-hedge quote suppression, and cancel-retry semantics are complete. Quality review passed with two deferred medium follow-ups: maker quote-age gating and operational use of `cancel_after_ms`. |
| Task 4: Add Fee-Aware Pricing And Explicit Fee/Wiring Observability | completed | main | `2026-03-16 09:34 UTC` fee-aware live decision math, exports, and maker-fill telemetry are in place. Verification: Task 4 slice `21 passed, 5 deselected`; full `tests/unit_tests/flux/strategies/makerv4` -> `87 passed`; `git diff --check` clean. |
| Task 5: Add MakerV4 `take_take` Mode With HL-First Aggression | completed | main | `2026-03-16 09:52 UTC` explicit `take_take` mode, fee-aware thresholds, cooldown, multi-fill aggregation, and pre-trade configured-size hedgeability gate are implemented. Verification: Task 5 slice `10 passed, 60 deselected`; full `tests/unit_tests/flux/strategies/makerv4` -> `97 passed`; `git diff --check` clean. Deferred known risk: partial maker fills whose final filled quantity remains below the hedgeable IBKR share increment, which is outside this wave’s stated partial/residual scope. |
| Task 6: Align Control-Plane Exports For Mode, Fees, And Hedge Policy | completed | main | `2026-03-16 10:11 UTC` backend operator payload now exposes execution mode, behavior, fee assumptions, and current hedge policy with correct precedence for pending hedge vs steady-state fallback. Verification: Task 6 slice `9 passed, 80 deselected`; `git diff --check` clean. |
| Task 7: Run Live Gates And One-Symbol MakerV4 Smoke In Regular And Overnight Contexts | blocked | main | `2026-03-16 10:15 UTC` live gate check result: `check_equities_live_readiness.sh --json` -> `ok=true`, but `sudo -n ... hyperliquid_request_quota.py --show-only` reports `cumVlm=5856.56 nRequestsUsed=17118 nRequestsCap=15856 nRequestsSurplus=0`. No MakerV4 live canary should start until request headroom is positive. |

---

### Task 1: Lock Overnight IBKR Hedge Policy And Fee Contract In Tests And Docs

**Files:**
- Modify: `tests/integration_tests/adapters/interactive_brokers/test_execution_order_transform.py`
- Modify: `tests/unit_tests/flux/strategies/makerv4/test_strategy.py`
- Modify: `tests/unit_tests/flux/strategies/makerv4/test_runtime_params.py`
- Modify: `deploy/equities/README.md`
- Modify: `docs/plans/2026-03-13-makerv4-hl-ibkr-prod-cutover.md`

**Step 1: Write failing tests for the overnight hedge contract**

Add tests that require:

- regular-session stock hedges still use the existing immediate hedge policy
- overnight stock hedges switch to `SMART + includeOvernight=true`
- overnight stock hedges do not use `IOC`
- the hedge policy carries enough metadata for a follow-up timed cancel path
- Makerv4 config/runtime allows explicit fee-plan assumptions without hardcoded account ids

Example test shape:

```python
def test_makerv4_overnight_smart_hedge_policy_uses_day_and_include_overnight():
    policy = build_ibkr_hedge_order_policy(
        instrument_id=InstrumentId.from_str("GOOGL.NASDAQ"),
        route="SMART",
        is_regular_session=False,
    )

    assert policy.time_in_force == TimeInForce.DAY
    assert policy.include_overnight is True
    assert policy.outside_rth is True
    assert policy.cancel_after_ms > 0
```

**Step 2: Run tests to verify failure**

Run:

```bash
./.venv/bin/pytest -q \
  tests/integration_tests/adapters/interactive_brokers/test_execution_order_transform.py \
  tests/unit_tests/flux/strategies/makerv4/test_strategy.py \
  tests/unit_tests/flux/strategies/makerv4/test_runtime_params.py \
  -k 'overnight or include_overnight or hedge_policy or fee_plan' \
  -p no:rerunfailures
```

Expected: FAIL because the current Makerv4 hedge path still assumes IOC-oriented behavior and does not expose a complete session-aware policy object.

**Step 3: Update deploy docs and cutover plan notes**

Record the production contract explicitly:

- for overnight-capable IBKR hedges, prefer `SMART`
- use `includeOvernight=true`
- do not use `IOC` on the overnight SMART stock route
- production target is `IBKR Pro Tiered`
- residual hedge management remains out of scope

**Step 4: Re-run the test slice after Task 3**

Expected: PASS.

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 2: Implement Shared Session-Aware IBKR Hedge Order Policy

**Files:**
- Create: `systems/flux/flux/strategies/shared/ibkr_order_policy.py`
- Modify: `systems/flux/flux/strategies/shared/ibkr_tags.py`
- Modify: `nautilus_trader/adapters/interactive_brokers/common.py`
- Modify: `tests/unit_tests/flux/strategies/shared/test_ibkr_order_policy.py`
- Modify: `tests/integration_tests/adapters/interactive_brokers/test_execution_order_transform.py`

**Step 1: Write the failing shared-policy tests**

Add tests that pin:

- regular-session policy
- overnight SMART policy
- tag generation including `outsideRth` and `includeOvernight`
- explicit cancel budget on overnight non-IOC hedges
- no account- or symbol-specific hardcodes

**Step 2: Run tests to verify failure**

Run:

```bash
./.venv/bin/pytest -q \
  tests/unit_tests/flux/strategies/shared/test_ibkr_order_policy.py \
  tests/integration_tests/adapters/interactive_brokers/test_execution_order_transform.py \
  -k 'order_policy or include_overnight or outside_rth' \
  -p no:rerunfailures
```

Expected: FAIL because no shared session-aware IBKR hedge policy module exists yet.

**Step 3: Implement the minimal shared policy**

Create a shared policy object, for example:

```python
@dataclass(frozen=True, slots=True)
class IbkrHedgeOrderPolicy:
    route: str
    tif: TimeInForce
    outside_rth: bool
    include_overnight: bool
    cancel_after_ms: int | None
```

The policy builder should derive these fields from:

- session state (`regular` vs `overnight`)
- configured route preference
- hedge mode (`maker_hedge` vs `take_take`)
- current product assumptions (US equities hedge)

**Step 4: Re-run the shared-policy tests**

Expected: PASS.

**Step 5: Commit**

```bash
git add \
  systems/flux/flux/strategies/shared/ibkr_order_policy.py \
  systems/flux/flux/strategies/shared/ibkr_tags.py \
  nautilus_trader/adapters/interactive_brokers/common.py \
  tests/unit_tests/flux/strategies/shared/test_ibkr_order_policy.py \
  tests/integration_tests/adapters/interactive_brokers/test_execution_order_transform.py
git commit -m "feat: add shared ibkr overnight hedge policy"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 3: Rewire MakerV4 Hedge Submission For Session-Aware Overnight Behavior

**Files:**
- Modify: `systems/flux/flux/strategies/makerv4/strategy.py`
- Modify: `systems/flux/flux/strategies/makerv4/managed_orders.py`
- Modify: `systems/flux/flux/strategies/makerv4/runtime_params.py`
- Modify: `tests/unit_tests/flux/strategies/makerv4/test_strategy.py`
- Modify: `tests/unit_tests/flux/strategies/makerv4/test_runtime_params.py`

**Step 1: Write failing Makerv4 hedge-lifecycle tests**

Add tests that require:

- regular-session hedge submission remains immediate
- overnight hedge submission uses the shared IBKR policy instead of hardcoded IOC
- overnight pending hedges arm a cancel timer/budget
- hedge order metadata includes route, TIF, and overnight flags in state exports
- partial hedge fills still fail closed for this wave

**Step 2: Run tests to verify failure**

Run:

```bash
./.venv/bin/pytest -q \
  tests/unit_tests/flux/strategies/makerv4/test_strategy.py \
  tests/unit_tests/flux/strategies/makerv4/test_runtime_params.py \
  -k 'overnight or hedge_policy or pending_hedge or cancel_after' \
  -p no:rerunfailures
```

Expected: FAIL because MakerV4 does not yet consume a shared session-aware hedge policy or track cancel budgets for overnight hedges.

**Step 3: Implement the minimal strategy changes**

Wire MakerV4 so `_submit_hedge_intent(...)`:

- builds the shared IBKR hedge policy
- submits `SMART + includeOvernight=true + DAY` overnight hedges when outside regular session
- stores `cancel_after_ms` and route/TIF metadata in pending hedge state
- preserves the current fail-closed behavior on reject/cancel/partial/timeout

Do not add residual hedge management.

**Step 4: Re-run the Makerv4 hedge-lifecycle tests**

Expected: PASS.

**Step 5: Run the existing Makerv4 regression bundle**

Run:

```bash
./.venv/bin/pytest -q \
  tests/unit_tests/flux/strategies/makerv4 \
  tests/unit_tests/examples/strategies/test_equities_run_node.py -k 'makerv4 or maker_v4' \
  -p no:rerunfailures
```

Expected: PASS.

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 4: Add Fee-Aware Pricing And Explicit Fee/Wiring Observability

**Files:**
- Modify: `systems/flux/flux/strategies/makerv4/pricing.py`
- Modify: `systems/flux/flux/strategies/makerv4/runtime_params.py`
- Modify: `systems/flux/flux/strategies/makerv4/strategy.py`
- Modify: `systems/flux/flux/strategies/makerv4/publisher.py`
- Modify: `tests/unit_tests/flux/strategies/makerv4/test_pricing.py`
- Modify: `tests/unit_tests/flux/strategies/makerv4/test_publisher_contract.py`
- Optional Create: `docs/plans/2026-03-16-makerv4-fee-pricing-notes.md`

**Step 1: Write failing pricing/observability tests**

Add tests that require:

- decision thresholds can include explicit fee assumptions
- fee assumptions are surfaced in strategy state / quote snapshot / hedge snapshot exports
- `take_take` and `maker_hedge` can use the same fee model inputs without mode-specific hardcodes

Suggested minimum knobs:

- `ibkr_fee_plan = "fixed" | "tiered"`
- `ibkr_fee_min_usd`
- `hl_taker_fee_bps`
- `hl_maker_fee_bps`

**Step 2: Run tests to verify failure**

Run:

```bash
./.venv/bin/pytest -q \
  tests/unit_tests/flux/strategies/makerv4/test_pricing.py \
  tests/unit_tests/flux/strategies/makerv4/test_publisher_contract.py \
  -k 'fee or pricing or threshold or quote_snapshot' \
  -p no:rerunfailures
```

Expected: FAIL because current Makerv4 pricing does not expose a clear fee contract for production basis or `take_take` thresholds.

**Step 3: Implement the minimal fee-aware pricing surface**

Add fee model inputs and make them visible in exported state. Keep the implementation intentionally simple:

- use configured fee assumptions
- keep actual realized commission reports separate
- do not add a full PnL or slippage engine in this wave

**Step 4: Re-run the pricing/observability tests**

Expected: PASS.

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 5: Add MakerV4 `take_take` Mode With HL-First Aggression

**Files:**
- Modify: `systems/flux/flux/strategies/makerv4/strategy.py`
- Modify: `systems/flux/flux/strategies/makerv4/pricing.py`
- Modify: `systems/flux/flux/strategies/makerv4/runtime_params.py`
- Modify: `systems/flux/flux/strategies/makerv4/publisher.py`
- Modify: `tests/unit_tests/flux/strategies/makerv4/test_strategy.py`
- Modify: `tests/unit_tests/flux/strategies/makerv4/test_pricing.py`
- Modify: `tests/unit_tests/flux/strategies/makerv4/test_runtime_params.py`

**Step 1: Write failing `take_take` tests**

Add tests that require:

- `execution_mode="take_take"` bypasses resting maker quote placement
- HL aggression occurs only when fee-aware spread thresholds are met
- separate knobs exist for `bid_edge_take_bps`, `ask_edge_take_bps`, and `take_cooldown_ms`
- after HL fill confirmation, the strategy submits exactly one IBKR hedge
- partial hedge behavior still fails closed for this wave

Example shape:

```python
def test_take_take_mode_aggresses_hl_only_when_bid_threshold_is_met():
    ...
    assert submitted_hl_order.order_side == OrderSide.BUY
    assert pending_hedge is None
```

**Step 2: Run tests to verify failure**

Run:

```bash
./.venv/bin/pytest -q \
  tests/unit_tests/flux/strategies/makerv4/test_strategy.py \
  tests/unit_tests/flux/strategies/makerv4/test_pricing.py \
  tests/unit_tests/flux/strategies/makerv4/test_runtime_params.py \
  -k 'take_take or execution_mode or cooldown' \
  -p no:rerunfailures
```

Expected: FAIL because MakerV4 currently only supports the maker quote path.

**Step 3: Implement the minimal `take_take` mode**

Add:

- `execution_mode`
- `bid_edge_take_bps`
- `ask_edge_take_bps`
- `take_cooldown_ms`

Behavior:

- no resting quote stack in `take_take`
- aggress HL first
- send IBKR hedge after HL fill confirmation
- do not attempt residual hedge management

Keep the existing maker path unchanged for `maker_hedge`.

**Step 4: Re-run the `take_take` tests**

Expected: PASS.

**Step 5: Run combined Makerv4 regression tests**

Run:

```bash
./.venv/bin/pytest -q tests/unit_tests/flux/strategies/makerv4 -p no:rerunfailures
```

Expected: PASS.

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 6: Align Control-Plane Exports For Mode, Fees, And Hedge Policy

**Files:**
- Modify: `systems/flux/flux/api/_payloads_signals.py`
- Modify: `systems/flux/flux/api/test_payloads.py`
- Modify: `tests/unit_tests/flux/api/test_equities_profile_contract.py`
- Optional Modify: `fluxboard/types.ts`
- Optional Modify: `fluxboard/stores.ts`

**Step 1: Write failing export-contract tests**

Add tests that require the backend signal payload to expose enough information for operators to understand:

- current `execution_mode`
- hedge route / TIF / overnight flags
- fee assumptions used by the strategy
- whether the strategy is using maker or take-take behavior

Do not scope full Fluxboard polish into this task. Backend contract first.

**Step 2: Run tests to verify failure**

Run:

```bash
./.venv/bin/pytest -q \
  tests/unit_tests/flux/api/test_payloads.py \
  tests/unit_tests/flux/api/test_equities_profile_contract.py \
  -k 'makerv4 or execution_mode or overnight or fee' \
  -p no:rerunfailures
```

Expected: FAIL because the current payload contract does not yet carry the full new mode/policy surface.

**Step 3: Implement the minimal export changes**

Update the signal payload/backend contract so later Fluxboard work can render the right state without reverse engineering strategy internals.

**Step 4: Re-run the export-contract tests**

Expected: PASS.

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 7: Run Live Gates And One-Symbol MakerV4 Smoke In Regular And Overnight Contexts

**Files:**
- Modify: `deploy/equities/strategies/aapl_tradexyz_makerv4.toml`
- Modify: `deploy/equities/README.md`
- Optional Modify: `ops/scripts/deploy/check_equities_live_readiness.sh`
- Optional Modify: `docs/plans/2026-03-12-equities-live-trading-readiness.md`

**Step 1: Confirm live gates before any canary**

Run:

```bash
./ops/scripts/deploy/check_equities_live_readiness.sh --json
sudo -n env PYTHONPATH=/home/ubuntu/nautilus_trader/.worktrees/equities-live-pr \
  /home/ubuntu/nautilus_trader/.worktrees/equities-live-pr/.venv/bin/python \
  /home/ubuntu/nautilus_trader/.worktrees/equities-live-pr/ops/scripts/deploy/hyperliquid_request_quota.py --show-only
```

Expected:

- readiness `ok=true`
- IBKR auth healthy
- Hyperliquid funded account has positive request headroom

If headroom is still zero, mark Task 7 `blocked` and stop.

**Step 2: Enable a one-symbol MakerV4 canary**

Use one symbol only, one-per-side, with explicit rollback to MakerV3.

**Step 3: Run two smoke tests**

- regular-session MakerV4 canary smoke
- overnight MakerV4 canary smoke using `SMART + includeOvernight=true` hedge behavior

For the overnight smoke, verify:

- HL order placement/acceptance
- IBKR hedge submission on fill confirmation
- IBKR fill/commission callbacks
- state exports reflect the overnight hedge policy

**Step 4: Capture evidence and update trackers**

Record:

- request headroom before/after
- fill/cancel outcomes
- commissions seen
- whether `take_take` remains disabled or tested separately

**Step 5: Commit**

```bash
git add \
  deploy/equities/strategies/aapl_tradexyz_makerv4.toml \
  deploy/equities/README.md \
  docs/plans/2026-03-12-equities-live-trading-readiness.md \
  docs/plans/2026-03-16-makerv4-take-take-and-overnight-hedge.md
git commit -m "feat: harden makerv4 overnight hedge policy and take-take mode"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

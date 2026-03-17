# MakerV4 HL-vs-IBKR Prod Cutover Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.
> **Progress:** The Progress Tracker in this document is the execution source of truth and must be updated on every state change.

**Goal:** Finish MakerV4 into a production-safe Hyperliquid-vs-IBKR basis strategy on the shared equities control plane, with live maker quoting on Hyperliquid, immediate IOC hedges on IBKR, and mixed-family `/equities` surfaces that stay consistent with Nautilus engine conventions. For this wave, the maker contract is explicitly one-band / one-quote-per-side unless a later task deliberately broadens it.

**Architecture:** Do not bolt ad hoc behavior onto the existing MakerV4 skeleton. Finish MakerV4 as a first-class strategy with two clearly separated responsibilities: maker-side quote management on Hyperliquid and fill-triggered immediate hedge submission on IBKR. For this cutover wave, strategy truth must come from actual maker-order lifecycle callbacks and the shared-account projection contract, not optimistic local bookkeeping or duplicate strategy-owned IBKR balance rows. Reuse existing generic ladder/pricing/inventory contracts where they are already strategy-family-agnostic, but do not copy Makerv3 internals blindly. Residual hedge management is intentionally out of scope for this wave; partial or missed hedges must fail closed and alert.

**Tech Stack:** Python strategy/runtime code, Nautilus Trader strategy lifecycle and order APIs, Flux runners/API/profile contracts, Redis-backed params and portfolio inventory feeds, IBKR and Hyperliquid venue adapters, pytest, live deploy TOMLs, systemd/Pulse services.

## March 16, 2026 Overnight Hedge Addendum

- Overnight-capable IBKR stock hedges prefer `SMART`.
- The overnight-capable SMART stock route must set `includeOvernight=true`.
- The overnight-capable SMART stock route must not use `IOC`.
- The production fee target is `IBKR Pro Tiered`, modeled as an explicit fee-plan assumption rather than an account-id-specific branch.
- Residual hedge management remains out of scope; partial or failed hedges still fail closed.

## Current Review Findings

1. MakerV4 is conceptually aligned with the intended basis model, but the current implementation is not live-ready.
2. MakerV4 now has a real one-per-side maker quote path, real IOC hedge submission, shared-account position reconciliation via the profile-account-projection feed, and maker-order reconciliation through fill plus terminal callbacks.
3. Corrected local/unit/API verification for this batch is green:
   - `tests/unit_tests/flux/strategies/makerv4` passes (`66 passed`).
   - `tests/unit_tests/examples/strategies/test_equities_run_node.py -k 'makerv4 or maker_v4'` passes (`7 passed, 21 deselected`).
   - `tests/unit_tests/flux/api/test_equities_profile_contract.py` passes (`28 passed`).
4. MakerV4 should not reintroduce strategy-owned IBKR reference balance rows. The runner/profile contract remains profile-owned for IBKR shared-account balances, and the mixed-family `/balances` path now preserves shared-account row ids and provenance even when `inventory_by_asset` is empty.
5. The current maker loop is explicitly one quote per side when `n_orders1 > 0`; that remains the contract for this wave and for any eventual canary.
6. Cross-stack follow-up hardening is now green in the worktree: fail-closed quote gating, maker managed-order lifecycle truth, shared-account position reconciliation, outside-RTH IBKR hedge tags, account-unique shared position row ids, and Hyperliquid quota tooling / venue-protection telemetry all have local verification.
7. Existing tests that monkeypatch `_submit_hedge_intent(...)` still need to be retired or supplemented where they hide production wiring, but that is now cleanup work rather than a blocker.
8. The remaining blockers are operational/live only: the IBKR gateway must finish auth recovery, Hyperliquid funded-account request headroom must be positively verified, and the first canary must stay one-per-side.

## Progress Tracker

**Source of truth:** Update this table whenever task state changes. Do not rely on memory, chat history, or TodoWrite alone.

| Task | Status | Owner | Notes / Last Update |
| --- | --- | --- | --- |
| Overall | blocked | main | `2026-03-16 06:16 UTC` reran the full Task 5 pre-gate bundle after the IBKR recovery note. Local verification is green (`84 passed, 47 deselected`), `./ops/scripts/deploy/check_equities_live_readiness.sh --json` is still `ok=true`, and `/api/v1/balances?profile=equities` remains `source=\"portfolio_snapshot_v2\"`, `degraded=false`, with no stale/missing/null required rows. Task 5 is still blocked only on Hyperliquid funded-account request headroom: `sudo -n env PYTHONPATH=/home/ubuntu/nautilus_trader/.worktrees/equities-live-pr /home/ubuntu/nautilus_trader/.worktrees/equities-live-pr/.venv/bin/python /home/ubuntu/nautilus_trader/.worktrees/equities-live-pr/ops/scripts/deploy/hyperliquid_request_quota.py --show-only` still reports `nRequestsSurplus=0`. |
| Task 1: Lock MakerV4 Review Findings And Live Contract In Tests | completed | main | `2026-03-13 18:39 UTC` wrong strategy-owned IBKR balance assertion rewritten around the profile-owned contract, mixed-family `/balances` provenance case fixed, and corrected test set is green. Verification: `test_strategy.py -k 'profile_owned or maker_order_cancel_reconciles'` `2 passed`; `test_inventory_contract.py -k 'shared_account_projection_when_local_cache_is_flat'` `1 passed`; `test_equities_profile_contract.py -k 'preserves_shared_account_provenance_when_makerv3_and_makerv4_coexist or makerv4 or mixed_family'` `3 passed`; full `test_equities_profile_contract.py` `28 passed`. |
| Task 2: Implement MakerV4 One-Per-Side Maker Quote Lifecycle And Order Reconciliation On The Shared Equities Control Plane | completed | main | `2026-03-13 18:53 UTC` maker-side fill events now reconcile the filled quote out of `_managed_maker_orders` before hedge handling, so state truth and the next refresh cycle both see the side as no longer open. Verification: maker-fill red slice `2 passed`; focused `test_strategy.py -k 'fill or reconcile or quote_state'` `12 passed`; full `tests/unit_tests/flux/strategies/makerv4` `66 passed`; mixed regression `tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py tests/unit_tests/flux/strategies/makerv4/test_strategy.py` `49 passed`. |
| Task 3: Wire Live IBKR IOC Hedge Submission And Hedge Lifecycle | completed | main | `2026-03-13 17:37 UTC` `_submit_hedge_intent(...)` now builds and submits a real IOC hedge order through Nautilus `order_factory.limit` + `submit_order`, and reject/cancel/expire hedge callbacks fail closed. Verification: hedge slice `17 passed, 12 deselected`. |
| Task 4: Reconcile MakerV4 Shared-Account Position Truth And Align Mixed-Family Balances, Params, Trades, And Signal Surfaces | completed | main | `2026-03-13 18:39 UTC` MakerV4 now exposes the profile-account-projection hook, uses shared-account HL positions when strategy-local cache is flat, and the mixed-family API path preserves shared-account provenance even when `inventory_by_asset` is empty. Verification: `test_inventory_contract.py -k 'shared_account_projection_when_local_cache_is_flat'` `1 passed`; `test_equities_run_node.py -k 'makerv4 or maker_v4'` `7 passed`; full `test_equities_profile_contract.py` `28 passed`; `test_payloads.py -k 'combine_portfolio_snapshot_rows'` `1 passed`. |
| Task 5: Enable One MakerV4 One-Per-Side Canary And Run Live HL-plus-IBKR Smoke | blocked | main | `2026-03-16 06:16 UTC` Task 5 remains blocked after a fresh rerun. Local pre-gate verification: `./.venv/bin/pytest -q tests/unit_tests/flux/strategies/makerv4 tests/unit_tests/examples/strategies/test_equities_run_node.py -k 'makerv4 or maker_v4' tests/unit_tests/flux/api/test_equities_profile_contract.py -k 'makerv4 or mixed_family' -p no:rerunfailures` -> `84 passed, 47 deselected`. Live checks remain healthy: `./ops/scripts/deploy/check_equities_live_readiness.sh --json` -> `ok=true`; `/api/v1/balances?profile=equities` -> `source=\"portfolio_snapshot_v2\"`, `degraded=false`, `stale_required=[]`. Hyperliquid funded-account quota remains the blocker: `sudo -n env PYTHONPATH=/home/ubuntu/nautilus_trader/.worktrees/equities-live-pr /home/ubuntu/nautilus_trader/.worktrees/equities-live-pr/.venv/bin/python /home/ubuntu/nautilus_trader/.worktrees/equities-live-pr/ops/scripts/deploy/hyperliquid_request_quota.py --show-only` -> `nRequestsUsed=17118`, `nRequestsCap=15856`, `nRequestsSurplus=0`. No deploy or canary switch was attempted. |

---

### Task 1: Lock MakerV4 Review Findings And Live Contract In Tests

**Files:**
- Modify: `tests/unit_tests/flux/strategies/makerv4/test_strategy.py`
- Modify: `tests/unit_tests/flux/strategies/makerv4/test_inventory_contract.py`
- Modify: `tests/unit_tests/examples/strategies/test_equities_run_node.py`
- Modify: `tests/unit_tests/flux/api/test_equities_profile_contract.py`
- Optional Modify: `tests/unit_tests/flux/strategies/makerv4/test_observability_and_exports.py`

**Step 1: Write the failing tests for the missing live contract**

Add tests that pin the intended production behavior:

- MakerV4 can place and manage Hyperliquid maker quotes when `bot_on=true` and market data is fresh.
- MakerV4 does not require monkeypatching `_submit_hedge_intent(...)` to create an IBKR IOC hedge order.
- MakerV4 state payloads expose maker-side managed orders and hedge-side pending orders distinctly.
- Mixed-family `/equities` API rows remain stable when MakerV3 and MakerV4 strategies coexist.
- Strategy/profile balance ownership is explicit: MakerV4 does not duplicate profile-owned IBKR reference balance rows in strategy-published balance snapshots, and the failing test is rewritten around the correct profile-owned assertion.

**Step 2: Run tests to verify they fail**

Run:

```bash
./.venv/bin/pytest -q \
  tests/unit_tests/flux/strategies/makerv4/test_strategy.py \
  tests/unit_tests/flux/strategies/makerv4/test_inventory_contract.py \
  tests/unit_tests/examples/strategies/test_equities_run_node.py -k 'makerv4 or maker_v4' \
  tests/unit_tests/flux/api/test_equities_profile_contract.py -k 'makerv4 or mixed_family' \
  -p no:rerunfailures
```

Expected:
- Existing Makerv4 balance test still fails.
- New live-maker / live-hedge tests fail because maker quote management and hedge submission are not wired yet.

**Step 3: Record go/no-go criteria in the tests and task notes**

The post-task bar is:

- MakerV4 has a real maker quote-management path.
- MakerV4 can submit a real IBKR IOC hedge intent through Nautilus order APIs.
- MakerV4 tests do not require duplicate strategy-owned IBKR balance rows.
- Mixed-family surfaces stay green under the shared equities profile.

**Step 4: Re-run the same tests after Task 4**

Expected: PASS.

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 2: Implement MakerV4 One-Per-Side Maker Quote Lifecycle And Order Reconciliation On The Shared Equities Control Plane

**Files:**
- Modify: `systems/flux/flux/strategies/makerv4/strategy.py`
- Modify: `systems/flux/flux/strategies/makerv4/runtime_params.py`
- Modify: `systems/flux/flux/strategies/makerv4/publisher.py`
- Modify: `systems/flux/flux/strategies/makerv4/constants.py`
- Optional Create: `systems/flux/flux/strategies/shared/maker_quote_runtime.py`
- Optional Modify: `systems/flux/flux/strategies/makerv3/quote_engine.py`
- Optional Modify: `systems/flux/flux/strategies/makerv3/rebalancing.py`
- Test: `tests/unit_tests/flux/strategies/makerv4/test_strategy.py`
- Test: `tests/unit_tests/flux/strategies/makerv4/test_pricing.py`
- Test: `tests/unit_tests/flux/strategies/makerv4/test_publisher_contract.py`

**Step 1: Add the failing MakerV4 quote-management tests**

Write tests that require:

- initial two-sided quote placement from fresh maker/reference quotes
- no quotes when bot is off or data is stale
- explicit one-band / one-quote-per-side contract in state/export surfaces for this wave
- managed-order state publication compatible with the shared `/equities` signal contract
- maker order rejected / canceled / expired callbacks reconcile managed-order truth instead of leaving optimistic local state behind
- quote replacement behavior that follows explicit thresholds rather than hardcoded venue-specific shortcuts

**Step 2: Run the new quote-management tests to confirm failure**

Run:

```bash
./.venv/bin/pytest -q \
  tests/unit_tests/flux/strategies/makerv4/test_strategy.py \
  tests/unit_tests/flux/strategies/makerv4/test_publisher_contract.py \
  tests/unit_tests/flux/strategies/makerv4/test_pricing.py \
  -k 'quote or managed or state_snapshot' \
  -p no:rerunfailures
```

Expected: FAIL because MakerV4 does not yet manage maker orders.

**Step 3: Implement the minimal maker-side lifecycle**

Add a real MakerV4 maker loop that:

- consumes the same fresh maker/reference quote inputs already cached by the strategy
- computes maker targets from runtime params without introducing new venue/account hardcodes
- uses Nautilus order APIs for maker order submission/cancel/reconciliation
- reconciles maker managed-order truth from actual order lifecycle callbacks rather than only local submission bookkeeping
- keeps the current wave contract explicit: one quote per side, not a hidden multi-level ladder
- publishes strategy truth into state snapshots and inventory components using the same profile-owned portfolio contracts already established for equities

Prefer shared abstractions only where they are truly family-agnostic. If extracting generic quote-ladder helpers from MakerV3 reduces duplication materially, do so in shared modules and keep MakerV3 behavior unchanged.

**Step 4: Re-run the quote-management tests**

Expected: PASS.

**Step 5: Run mixed regression tests against MakerV3 to ensure no accidental breakage**

Run:

```bash
./.venv/bin/pytest -q \
  tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py \
  tests/unit_tests/flux/strategies/makerv4/test_strategy.py \
  -p no:rerunfailures
```

Expected: PASS.

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 3: Wire Live IBKR IOC Hedge Submission And Hedge Lifecycle

**Files:**
- Modify: `systems/flux/flux/strategies/makerv4/strategy.py`
- Modify: `systems/flux/flux/strategies/makerv4/managed_orders.py`
- Modify: `systems/flux/flux/strategies/makerv4/market_data.py`
- Modify: `systems/flux/flux/strategies/makerv4/pricing.py`
- Optional Modify: `systems/flux/flux/strategies/shared/publisher_common.py`
- Test: `tests/unit_tests/flux/strategies/makerv4/test_strategy.py`
- Test: `tests/unit_tests/flux/strategies/makerv4/test_runtime_params.py`

**Step 1: Add failing tests for real hedge submission**

Write tests that require:

- `_submit_hedge_intent(...)` creates a real Nautilus order and submits it
- hedge route/instrument selection follows config/runtime metadata rather than hardcoded symbols
- partial hedge fills fail closed and surface a clear disabled reason
- hedge order reject / cancel / timeout paths fail closed and alert
- duplicate maker fills do not double-submit hedges

**Step 2: Run the hedge tests to verify failure**

Run:

```bash
./.venv/bin/pytest -q \
  tests/unit_tests/flux/strategies/makerv4/test_strategy.py \
  tests/unit_tests/flux/strategies/makerv4/test_runtime_params.py \
  -k 'hedge or fill' \
  -p no:rerunfailures
```

Expected: FAIL because `_submit_hedge_intent(...)` is currently a stub and hedge lifecycle callbacks are incomplete.

**Step 3: Implement the real hedge order path**

Finish the immediate-hedge loop so MakerV4:

- submits IBKR IOC hedge orders through the standard Nautilus strategy order API
- tracks pending hedge order ids and lifecycle events
- applies hedge execution reports from actual order callbacks
- fails closed on stale quotes, invalid hedge limit, reject, cancel, timeout, or partial fill

Residual hedge management remains out of scope. A partial or failed hedge must stop trading and alert rather than trying to work residual inventory.

**Step 4: Re-run the hedge tests**

Expected: PASS.

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 4: Reconcile MakerV4 Shared-Account Position Truth And Align Mixed-Family Balances, Params, Trades, And Signal Surfaces

**Files:**
- Modify: `systems/flux/flux/strategies/makerv4/strategy.py`
- Optional Modify: `systems/flux/flux/strategies/shared/account_projection_positions.py`
- Modify: `systems/flux/flux/strategies/makerv4/reference_balances.py`
- Modify: `systems/flux/flux/runners/equities/run_node.py`
- Modify: `systems/flux/flux/runners/equities/run_api.py`
- Modify: `systems/flux/flux/api/app.py`
- Modify: `systems/flux/flux/api/_payloads_balances.py`
- Optional Modify: `fluxboard/api.ts`
- Optional Modify: `fluxboard/api.flux.test.ts`
- Test: `tests/unit_tests/flux/strategies/makerv4/test_strategy.py`
- Test: `tests/unit_tests/examples/strategies/test_equities_run_node.py`
- Test: `tests/unit_tests/flux/api/test_equities_profile_contract.py`

**Step 1: Fix the existing failing Makerv4 balances/provider test**

Decide the correct production contract, then make the tests say that clearly:

- rewrite the current failing strategy test around the profile-owned balances contract
- do not reintroduce duplicate strategy-owned IBKR reference snapshot rows for MakerV4 observability

Do not keep ambiguous mixed ownership.

**Step 2: Add failing shared-account and mixed-family contract tests where needed**

Add or extend tests to pin:

- `run_node` attaches the profile account projection feed for MakerV4 when the shared equities projection path is enabled
- MakerV4 strategy truth (`local_qty`, `position_qty`, `inventory_source`, related signal fields) can reconcile the matching shared Hyperliquid account position through the existing profile-account-projection Redis contract
- `params` surface resolves `makerv4` param defaults and schema correctly
- `signals` surface includes MakerV4 quote/hedge-specific fields without breaking shared fields used by Fluxboard
- `balances` and `trades` surfaces remain stable when MakerV3 and MakerV4 coexist, including `portfolio_snapshot_v2` source/provenance under mixed-family `/balances`

**Step 3: Run the mixed-family/API tests to confirm failure**

Run:

```bash
./.venv/bin/pytest -q \
  tests/unit_tests/flux/strategies/makerv4/test_strategy.py \
  tests/unit_tests/examples/strategies/test_equities_run_node.py -k 'makerv4 or maker_v4' \
  tests/unit_tests/flux/api/test_equities_profile_contract.py -k 'makerv4 or mixed_family' \
  -p no:rerunfailures
```

Expected: FAIL until the balance contract, shared-account projection wiring, and mixed-family payload expectations are aligned.

**Step 4: Implement the minimal integration fixes**

Keep the shared equities control plane canonical:

- add the missing MakerV4 profile-account-projection hook and shared-position reconciliation path using the existing shared-account projection contract
- no strategy-specific hardcoding in API routes
- no duplicate balance ownership between profile snapshot and strategy supplement
- no optimistic maker/shared-position truth that can disagree with live shared-account state
- no Fluxboard-only masking of backend drift

**Step 5: Re-run the mixed-family/API tests**

Expected: PASS.

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 5: Enable One MakerV4 One-Per-Side Canary And Run Live HL-plus-IBKR Smoke

**Files:**
- Modify: `deploy/equities/strategies/aapl_tradexyz_makerv4.toml.disabled`
- Modify: `deploy/equities/equities.live.toml`
- Modify: `deploy/equities/README.md`
- Modify: `docs/plans/2026-03-12-equities-live-trading-readiness.md`
- Optional Modify: `ops/scripts/deploy/check_equities_live_readiness.sh`
- Test/Verify: live `signals`, `balances`, `params`, `trades`, journald, and IBKR/HL account-state checks

**Step 1: Prepare the canary config**

Switch exactly one retained symbol to MakerV4 canary mode, keeping:

- the same shared equities profile
- the same shared account/control-plane contracts
- the explicit one-band / one-quote-per-side maker contract for this wave
- explicit rollback path back to MakerV3

**Step 2: Ensure Hyperliquid request headroom exists before live smoke**

Do not start the live canary over a known `nRequestsSurplus=0` account. The pre-smoke operational gate is:

- positive Hyperliquid request headroom on the funded master account
- explicit operator note on how that headroom was obtained

**Step 3: Run the local verification bundle**

Run:

```bash
./.venv/bin/pytest -q \
  tests/unit_tests/flux/strategies/makerv4 \
  tests/unit_tests/examples/strategies/test_equities_run_node.py -k 'makerv4 or maker_v4' \
  tests/unit_tests/flux/api/test_equities_profile_contract.py -k 'makerv4 or mixed_family' \
  -p no:rerunfailures
```

Expected: PASS.

**Step 4: Run the live smoke**

Verify all of the following for the single MakerV4 canary:

- fresh Hyperliquid maker data
- fresh IBKR reference data
- exactly one maker quote per side rests on book when the strategy is healthy
- a maker fill creates one IBKR IOC hedge order
- full hedge fill clears pending hedge state
- reject/partial/missed hedge fails closed and surfaces clearly in `signals`, logs, and alerts
- `/equities` `balances`, `params`, `trades`, and `signal` views stay coherent

Suggested live checks:

```bash
curl -fsS 'http://127.0.0.1:5024/api/v1/signals?profile=equities' | jq '.data.strategies[] | select(.meta.strategy_id | contains("makerv4"))'
curl -fsS 'http://127.0.0.1:5024/api/v1/balances?profile=equities' | jq '.data'
curl -fsS 'http://127.0.0.1:5024/api/v1/params?profile=equities' | jq '.data[] | select(.strategy_id | contains("makerv4"))'
sudo journalctl -u 'flux@equities-node-*' --since '10 minutes ago' --no-pager | rg 'makerv4|hedge|IOC|OrderFilled|OrderRejected' -S
```

**Step 5: Update the readiness tracker and stop for operator review**

Record:

- canary result
- any hedge miss / partial behavior
- remaining blockers before broader cutover

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

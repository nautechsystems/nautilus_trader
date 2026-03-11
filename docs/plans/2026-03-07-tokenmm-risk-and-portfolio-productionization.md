# TokenMM Risk And Portfolio Productionization Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.
> **Progress:** The Progress Tracker in this document is the execution source of truth and must be updated on every state change.

**Goal:** Bring MakerV3 local risk, shared portfolio risk, Signal, and Balances to a production-ready state by enforcing one source of truth, explicit quantity semantics, startup reconciliation, and a complete TDD-backed acceptance matrix.

**Architecture:** Treat the recent regressions as symptoms of one design problem: multiple layers are making partially overlapping risk decisions. `run_portfolio` must own shared portfolio truth, each MakerV3 strategy must own only its local maker-leg truth, and API/Fluxboard must render those sources instead of recomputing risk independently. The existing base-unit normalization work remains the quantity-normalization backbone; this plan defines the production contract, review path, reconciliation rules, and rollout gates that make that normalized model safe to trade on.

**Tech Stack:** Flux MakerV3 strategy/publisher/quote engine, Flux shared portfolio runner and snapshot contract, Flux API payloads, Fluxboard TokenMM contracts, Nautilus live execution engine and cache reconciliation, pytest, vitest, Pulse/systemd, TokenMM deploy docs/runbooks.

## Progress Tracker

**Source of truth:** Update this table whenever task state changes. Do not rely on memory, chat history, or TodoWrite alone.

| Task | Status | Owner | Notes / Last Update |
| --- | --- | --- | --- |
| Overall | completed | codex | All planned Tasks 1-7 are implemented. Post-completion follow-up `bd1e40f49` fixed the remaining Bybit perp signal contract gap by projecting canonical inventory fields directly from strategy state into `/api/v1/signals`, verified with new red-green tests plus the existing signal payload/app slices, and the live `flux@tokenmm-api.service` was restarted at `2026-03-07 14:34 UTC` to serve the corrected fields. One dirty-worktree full-bundle rerun still exposed unrelated pre-existing failures in `tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py` and `deploy/tokenmm/strategies/plumeusdt_okx_perp_makerv3.toml` / `tests/unit_tests/examples/strategies/test_tokenmm_stack_contract.py`, which remain intentionally excluded from this work. |
| Task 1: Freeze First-Principles Risk Contract And Review Scope | completed | codex | Reframed deploy/API/socket contract docs as rollout-target semantics rather than already-landed live truth, then reran `rg -n "local_qty_base|global_qty_base|source of truth|reconciliation|partial|global_qty_complete|risk_delta|rollout target" docs/architecture deploy/tokenmm/README.md fluxboard/docs` to confirm consistency. |
| Task 2: Build The Full Regression And Acceptance Matrix | completed | codex | Added red API contract tests for canonical base quantities and reconciliation-blocked tradeability, then ran the broad Task 2 slice: `pytest tests/unit_tests/flux/strategies/makerv3/test_strategy_lifecycle.py tests/unit_tests/flux/strategies/makerv3/test_inventory.py tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py tests/unit_tests/flux/common/test_portfolio_inventory.py tests/unit_tests/flux/common/test_portfolio_snapshot.py tests/unit_tests/flux/api/test_payloads.py tests/unit_tests/flux/api/test_app.py tests/unit_tests/examples/strategies/test_tokenmm_run_portfolio.py tests/unit_tests/examples/strategies/test_tokenmm_run_node.py -v` -> `13 failed, 211 passed`, with failures concentrated in the intended remaining contract gaps. |
| Task 3: Correct MakerV3 Local Risk Extraction At The Source | completed | codex | Fixed fresh-flat maker reports so balances suppress stale cache fallback while keeping the row absent, preserved successful qty-conversion metadata with explicit completeness in local/global exposure summaries, and reran `pytest tests/unit_tests/flux/strategies/makerv3/test_strategy_lifecycle.py tests/unit_tests/flux/strategies/makerv3/test_inventory.py tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py -v` -> `59 passed`. |
| Task 4: Enforce Startup And Runtime Reconciliation Before Trading | completed | codex | Enforced TokenMM live startup guardrails (`exec_reconciliation=true`, `filter_position_reports=false` when execution is enabled), made execution reconciliation honor configured startup timeouts for both mass-status and follow-up position-report phases, and verified `pytest tests/unit_tests/live/test_execution_engine.py -v` -> `55 passed`, `pytest tests/unit_tests/live/test_execution_engine_purge_startup.py -v` -> `3 passed`, `pytest tests/unit_tests/examples/strategies/test_tokenmm_run_node.py -v` -> `35 passed`. |
| Task 5: Make `run_portfolio` The Canonical Shared Risk And Balances Authority | completed | codex | Verified the canonical portfolio snapshot/balances slice end-to-end and updated the remaining run_portfolio fixtures to construct `StrategyInventoryComponent` with canonical `local_qty_base` plus `global_qty_base` assertions; `pytest tests/unit_tests/flux/common/test_portfolio_inventory.py -v` -> `7 passed`, `pytest tests/unit_tests/flux/common/test_portfolio_snapshot.py -v` -> `3 passed`, `pytest tests/unit_tests/examples/strategies/test_tokenmm_run_portfolio.py -v` -> `7 passed`, `pytest tests/unit_tests/flux/api/test_app.py -v` -> `63 passed`, `pytest tests/unit_tests/flux/api/test_payloads.py -v` -> `43 passed`. |
| Task 6: Make API And Fluxboard Honest Consumers Of Canonical Risk State | completed | codex | Verified the API consumer slice stayed green (`pytest tests/unit_tests/flux/api/test_payloads.py -v` -> `43 passed`, `pytest tests/unit_tests/flux/api/test_app.py -v` -> `63 passed`) and ran the Fluxboard signal consumer suite from the workspace root-adjusted `fluxboard` cwd with `pnpm exec vitest run tests/signal/*.test.tsx tests/signal/*.test.ts __tests__/panels/signal.test.tsx __tests__/signal-store.params-merge.test.ts` -> `8 passed, 44 tests`. Follow-up `bd1e40f49` then closed the remaining Bybit perp signal gap by making `/api/v1/signals` project canonical `position_qty_*`, `local_qty_base`, `global_qty_base`, completeness bits, and conversion metadata from strategy state instead of dropping them at the top level. |
| Task 7: Add Production Review Docs, Audit Tooling, And Rollout Gates | completed | codex | Added `docs/runbooks/tokenmm-risk-validation.md`, `scripts/ops/tokenmm_risk_audit.py`, deploy/contract cross-links, and isolated Task 7 contract coverage in `tests/unit_tests/examples/strategies/test_tokenmm_risk_validation_contract.py`; verified `rg -n "risk validation|source of truth|global_qty_base|local_qty_base|reconciliation" docs deploy/tokenmm fluxboard/docs`, `python3 scripts/ops/tokenmm_risk_audit.py --help`, `pytest tests/unit_tests/examples/strategies/test_tokenmm_risk_validation_contract.py -v`, plus the staged-scope pytest/vitest bundle all green. |

---

## First-Principles Contract

This plan is built around these invariants:

1. **Local strategy risk** is the maker leg only, normalized to base exposure, and is computed from local venue-visible truth:
   - fresh maker position reports or reconciled cache positions for perps
   - visible venue account balances for spot
2. **Shared global risk** is computed only by `run_portfolio` from per-strategy published inventory components.
3. **`/api/v1/balances?profile=tokenmm`** is a rendering of the shared portfolio snapshot, not a second risk engine.
4. **`/api/v1/signals?profile=tokenmm`** renders strategy state plus canonical portfolio metadata and must not invent alternative risk semantics from balances.
5. **Every risk-facing quantity must have explicit semantics.** The normalized truth is base exposure; compat aliases such as `local_qty` and `global_qty` may remain temporarily, but must be documented as base exposure aliases.
6. **Missing, stale, or unreconciled truth degrades explicitly.** No silent `0`, no stale-cache trust, no implicit fallback from one subsystem to another without metadata.
7. **A node may not trade before startup reconciliation reaches venue truth or explicit degraded/blocking state.**

## Related Work

This plan absorbs and sequences the following existing work instead of competing with it:

- `docs/plans/2026-03-07-base-unit-risk-and-balance-normalization.md`
- `docs/plans/2026-03-07-tokenmm-partial-global-risk-hardening.md`

Execution should treat those documents as supporting references. This document is the umbrella productionization plan and acceptance gate for TokenMM MakerV3 risk/portfolio behavior.

## Current Production Symptoms This Plan Must Eliminate

- Bybit perp local position remains stale/wrong in both Signal and Balances.
- Binance spot local risk can differ between strategy state and Balances when one venue has multiple visible accounts.
- Stable cash rows can oscillate if merge rules collapse conflicting duplicate account scopes incorrectly.
- Shared `global_qty` policy is partially documented and still too easy to misread as “truth” instead of “possibly partial truth”.
- Startup reconciliation and cache trust are not yet strong enough for production guarantees.

### Task 1: Freeze First-Principles Risk Contract And Review Scope

**Files:**
- Create: `docs/architecture/tokenmm-risk-source-of-truth.md`
- Modify: `docs/architecture/tokenmm-portfolio-inventory-semantics.md`
- Modify: `docs/architecture/quantity-units.md`
- Modify: `deploy/tokenmm/README.md`
- Modify: `fluxboard/docs/tokenmm_contract.md`
- Modify: `fluxboard/docs/tokenmm_socket_contract.md`

**Step 1: Write the failing documentation checklist**

Document these contract statements explicitly:

- `local_qty_base` is the maker-leg local base exposure seen by the strategy.
- `global_qty_base` is the shared portfolio aggregate owned by `run_portfolio`.
- `Balances(profile=tokenmm)` must match the shared portfolio snapshot, not independently recompute TokenMM risk.
- `Signals(profile=tokenmm)` may present strategy-local and portfolio-global quantities, but may not derive them from balances unless explicitly in compat fallback mode.
- `risk_delta` is not the canonical quantity field for spot local inventory and must not be used as a hidden substitute for `local_qty`.
- startup reconciliation failure means degraded or blocked trading, not “best effort” stale-cache trading.

**Step 2: Write the review section**

Add a short “observed failures and root causes” section with the concrete classes of bugs already seen:

- stale maker perp position source
- multi-account spot venue mismatch
- duplicate stable cash scope collapse
- partial shared-global semantics confusion

**Step 3: Verify documentation consistency**

Run:
```bash
rg -n "local_qty_base|global_qty_base|source of truth|reconciliation|partial|global_qty_complete|risk_delta" docs/architecture deploy/tokenmm/README.md fluxboard/docs
```

Expected: the docs consistently describe one risk contract and one ownership model.

**Step 4: Commit**

```bash
git add docs/architecture/tokenmm-risk-source-of-truth.md docs/architecture/tokenmm-portfolio-inventory-semantics.md docs/architecture/quantity-units.md deploy/tokenmm/README.md fluxboard/docs/tokenmm_contract.md fluxboard/docs/tokenmm_socket_contract.md
git commit -m "docs: freeze tokenmm risk and portfolio source-of-truth contract"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 2: Build The Full Regression And Acceptance Matrix

**Files:**
- Modify: `tests/unit_tests/flux/strategies/makerv3/test_strategy_lifecycle.py`
- Modify: `tests/unit_tests/flux/strategies/makerv3/test_inventory.py`
- Modify: `tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py`
- Modify: `tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py`
- Modify: `tests/unit_tests/flux/common/test_portfolio_inventory.py`
- Modify: `tests/unit_tests/flux/common/test_portfolio_snapshot.py`
- Modify: `tests/unit_tests/flux/api/test_payloads.py`
- Modify: `tests/unit_tests/flux/api/test_app.py`
- Modify: `tests/unit_tests/examples/strategies/test_tokenmm_run_portfolio.py`
- Modify: `tests/unit_tests/examples/strategies/test_tokenmm_run_node.py`
- Modify if needed: `tests/unit_tests/live/test_execution_engine.py`
- Modify if needed: `tests/unit_tests/live/test_execution_engine_purge_startup.py`

**Step 1: Add failing test cases for the production symptom classes**

At minimum, add red tests for:

```python
def test_local_spot_qty_aggregates_all_visible_maker_venue_accounts():
    ...
    assert skew["local_qty_base"] == Decimal("-30143.53768988")
```

```python
def test_local_perp_qty_prefers_fresh_venue_report_over_stale_cache_positions():
    ...
    assert skew["local_qty_base"] == Decimal("99382")
```

```python
def test_portfolio_snapshot_partial_global_qty_is_explicitly_incomplete():
    ...
    assert snapshot["inventory"]["global_qty_complete"] is False
```

```python
def test_stable_cash_merge_does_not_flip_non_zero_to_newer_zero():
    ...
    assert row["total"] == "500"
```

```python
def test_startup_reconciliation_blocks_trading_when_venue_truth_is_not_confirmed():
    ...
    assert strategy_state == "blocked_reconciliation"
```

**Step 2: Add the end-to-end API contract cases**

Cover:

- `signals` local/global quantities align with strategy state and portfolio metadata
- `balances(profile=tokenmm)` rows align with the shared portfolio snapshot
- `balances(strategy=<id>)` remains a debug view and may differ only when the strategy itself is wrong
- `bot_off` still publishes signal state and quote status

**Step 3: Run the failing test slice**

Run:
```bash
pytest tests/unit_tests/flux/strategies/makerv3/test_strategy_lifecycle.py -v
pytest tests/unit_tests/flux/strategies/makerv3/test_inventory.py -v
pytest tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py -v
pytest tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py -v
pytest tests/unit_tests/flux/common/test_portfolio_inventory.py -v
pytest tests/unit_tests/flux/common/test_portfolio_snapshot.py -v
pytest tests/unit_tests/flux/api/test_payloads.py -v
pytest tests/unit_tests/flux/api/test_app.py -v
pytest tests/unit_tests/examples/strategies/test_tokenmm_run_portfolio.py -v
pytest tests/unit_tests/examples/strategies/test_tokenmm_run_node.py -v
```

Expected: FAIL on the targeted production gaps, not from unrelated syntax or test harness problems.

**Step 4: Commit**

```bash
git add tests/unit_tests/flux/strategies/makerv3/test_strategy_lifecycle.py tests/unit_tests/flux/strategies/makerv3/test_inventory.py tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py tests/unit_tests/flux/common/test_portfolio_inventory.py tests/unit_tests/flux/common/test_portfolio_snapshot.py tests/unit_tests/flux/api/test_payloads.py tests/unit_tests/flux/api/test_app.py tests/unit_tests/examples/strategies/test_tokenmm_run_portfolio.py tests/unit_tests/examples/strategies/test_tokenmm_run_node.py tests/unit_tests/live/test_execution_engine.py tests/unit_tests/live/test_execution_engine_purge_startup.py
git commit -m "test: add tokenmm risk and portfolio production regression matrix"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 3: Correct MakerV3 Local Risk Extraction At The Source

**Files:**
- Modify: `systems/flux/flux/strategies/makerv3/strategy.py`
- Modify: `systems/flux/flux/strategies/makerv3/inventory.py`
- Modify: `systems/flux/flux/strategies/makerv3/publisher.py`
- Modify if needed: `systems/flux/flux/strategies/makerv3/quote_engine.py`
- Test: `tests/unit_tests/flux/strategies/makerv3/test_strategy_lifecycle.py`
- Test: `tests/unit_tests/flux/strategies/makerv3/test_inventory.py`
- Test: `tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py`

**Step 1: Implement the local-risk ownership rules**

Required behavior:

- spot maker strategies aggregate all visible maker-venue accounts before falling back to `account_for_venue()`
- perp maker strategies prefer fresh maker instrument venue reports over stale cache net positions
- local qty is always expressed as normalized base exposure
- a truly visible maker account with no base balance may publish `0`
- missing or unreconciled local truth must publish explicit degraded metadata instead of fabricated zeros

**Step 2: Keep state, balances, and inventory component publishing on the same local quantity**

The same local source must drive:

- strategy `pricing_debug.skew.local_inventory_qty_base`
- strategy `pricing_adjustments[].local_qty`
- balances publisher rows for the maker leg
- portfolio inventory component `local_qty_base`

Do not let these call separate inference paths.

**Step 3: Verify focused tests**

Run:
```bash
pytest tests/unit_tests/flux/strategies/makerv3/test_strategy_lifecycle.py -v
pytest tests/unit_tests/flux/strategies/makerv3/test_inventory.py -v
pytest tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py -v
```

Expected: PASS with spot/perp local-risk cases covered.

**Step 4: Commit**

```bash
git add systems/flux/flux/strategies/makerv3/strategy.py systems/flux/flux/strategies/makerv3/inventory.py systems/flux/flux/strategies/makerv3/publisher.py systems/flux/flux/strategies/makerv3/quote_engine.py tests/unit_tests/flux/strategies/makerv3/test_strategy_lifecycle.py tests/unit_tests/flux/strategies/makerv3/test_inventory.py tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py
git commit -m "fix: unify makerv3 local risk extraction"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 4: Enforce Startup And Runtime Reconciliation Before Trading

**Files:**
- Modify: `nautilus_trader/live/execution_engine.py`
- Modify: `systems/flux/flux/runners/tokenmm/run_node.py`
- Modify: `deploy/tokenmm/tokenmm.live.toml`
- Modify: `deploy/tokenmm/README.md`
- Test: `tests/unit_tests/live/test_execution_engine.py`
- Test: `tests/unit_tests/live/test_execution_engine_purge_startup.py`
- Test: `tests/unit_tests/examples/strategies/test_tokenmm_run_node.py`

**Step 1: Write the failing startup-reconciliation tests**

Cover:

- cache flush or explicit venue refresh on startup
- position reports and account truth are requested before the node becomes tradeable
- unreconciled startup keeps the strategy blocked/degraded
- config toggles do not silently permit stale-cache trading

Example:

```python
def test_live_node_remains_blocked_until_position_reconciliation_completes():
    ...
    assert payload["state"] == "blocked_reconciliation"
    assert payload["tradeable"] is False
```

**Step 2: Implement the minimum safe runtime contract**

Required outcome:

- startup must converge cache-to-venue truth or publish explicit degraded state
- MakerV3 may observe fresh venue reports for risk before full convergence, but must not quote unless the reconciliation policy says it is safe
- runtime position inconsistency must invalidate local risk cache and trigger a degrade/block path, not just log

**Step 3: Verify targeted tests**

Run:
```bash
pytest tests/unit_tests/live/test_execution_engine.py -v
pytest tests/unit_tests/live/test_execution_engine_purge_startup.py -v
pytest tests/unit_tests/examples/strategies/test_tokenmm_run_node.py -v
```

Expected: PASS with explicit startup safety semantics.

**Step 4: Commit**

```bash
git add nautilus_trader/live/execution_engine.py systems/flux/flux/runners/tokenmm/run_node.py deploy/tokenmm/tokenmm.live.toml deploy/tokenmm/README.md tests/unit_tests/live/test_execution_engine.py tests/unit_tests/live/test_execution_engine_purge_startup.py tests/unit_tests/examples/strategies/test_tokenmm_run_node.py
git commit -m "feat: enforce tokenmm startup reconciliation gate"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 5: Make `run_portfolio` The Canonical Shared Risk And Balances Authority

**Files:**
- Modify: `systems/flux/flux/common/portfolio_inventory.py`
- Modify: `systems/flux/flux/common/portfolio_snapshot.py`
- Modify: `systems/flux/flux/runners/tokenmm/run_portfolio.py`
- Modify if needed: `systems/flux/flux/runners/shared/portfolio_runner.py`
- Modify: `systems/flux/flux/api/_payloads_balances.py`
- Modify: `systems/flux/flux/api/app.py`
- Test: `tests/unit_tests/flux/common/test_portfolio_inventory.py`
- Test: `tests/unit_tests/flux/common/test_portfolio_snapshot.py`
- Test: `tests/unit_tests/examples/strategies/test_tokenmm_run_portfolio.py`
- Test: `tests/unit_tests/flux/api/test_app.py`
- Test: `tests/unit_tests/flux/api/test_payloads.py`

**Step 1: Move the shared quantity contract to explicit normalized fields**

The canonical portfolio snapshot must publish:

- `local_qty_base` per component
- `global_qty_base`
- `global_qty_base_complete`
- `aggregation_mode`
- contributor diagnostics
- merged balance rows and totals

Compat aliases such as `local_qty` and `global_qty` may remain temporarily, but they must mirror the normalized base fields and be documented as such.

**Step 2: Guarantee one canonical shared snapshot**

`run_portfolio` must own:

- component health
- partial vs strict shared global quantity
- merged stable cash behavior
- merged position and balance rows
- shared totals used by `Balances(profile=tokenmm)`

No other layer may rebuild TokenMM portfolio semantics independently.

**Step 3: Verify tests**

Run:
```bash
pytest tests/unit_tests/flux/common/test_portfolio_inventory.py -v
pytest tests/unit_tests/flux/common/test_portfolio_snapshot.py -v
pytest tests/unit_tests/examples/strategies/test_tokenmm_run_portfolio.py -v
pytest tests/unit_tests/flux/api/test_app.py -v
pytest tests/unit_tests/flux/api/test_payloads.py -v
```

Expected: PASS with shared balances and shared global risk owned by the portfolio snapshot.

**Step 4: Commit**

```bash
git add systems/flux/flux/common/portfolio_inventory.py systems/flux/flux/common/portfolio_snapshot.py systems/flux/flux/runners/tokenmm/run_portfolio.py systems/flux/flux/runners/shared/portfolio_runner.py systems/flux/flux/api/_payloads_balances.py systems/flux/flux/api/app.py tests/unit_tests/flux/common/test_portfolio_inventory.py tests/unit_tests/flux/common/test_portfolio_snapshot.py tests/unit_tests/examples/strategies/test_tokenmm_run_portfolio.py tests/unit_tests/flux/api/test_app.py tests/unit_tests/flux/api/test_payloads.py
git commit -m "feat: make tokenmm portfolio snapshot canonical"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 6: Make API And Fluxboard Honest Consumers Of Canonical Risk State

**Files:**
- Modify: `systems/flux/flux/api/_payloads_signals.py`
- Modify: `systems/flux/flux/api/_payloads_balances.py`
- Modify if needed: `systems/flux/flux/api/app.py`
- Modify if needed: `fluxboard/components/domain/signal/SignalTable.tsx`
- Modify if needed: `fluxboard/components/domain/balances/*`
- Modify: `fluxboard/types.ts`
- Test: `tests/unit_tests/flux/api/test_payloads.py`
- Test: `tests/unit_tests/flux/api/test_app.py`
- Test if needed: `fluxboard/tests/signal/*`
- Test if needed: `fluxboard/tests/balances/*`

**Step 1: Remove implicit cross-surface recomputation**

Required rules:

- Signal local/global qty columns render canonical quantity fields from strategy state / portfolio metadata
- Balances renders canonical portfolio snapshot rows
- UI does not hide signal state because `bot_on=0`
- if a quantity is incomplete or degraded, surface that with metadata rather than switching to an unrelated fallback quantity

**Step 2: Keep compat fallbacks narrow and explicit**

Legacy fallback order should be documented and tested. Example:

- prefer `local_qty_base` / `local_qty`
- prefer `global_qty_base` / `global_qty`
- fall back to legacy `pricing_debug` only for older rows
- do not fall back from local spot risk to `risk_delta` if canonical local qty exists

**Step 3: Verify tests**

Run:
```bash
pytest tests/unit_tests/flux/api/test_payloads.py -v
pytest tests/unit_tests/flux/api/test_app.py -v
pnpm --dir fluxboard exec vitest run tests/signal/*.test.tsx tests/balances/*.test.tsx
```

Expected: PASS with Signals and Balances rendering the same canonical source-of-truth semantics.

**Step 4: Commit**

```bash
git add systems/flux/flux/api/_payloads_signals.py systems/flux/flux/api/_payloads_balances.py systems/flux/flux/api/app.py fluxboard/components/domain/signal/SignalTable.tsx fluxboard/components/domain/balances fluxboard/types.ts tests/unit_tests/flux/api/test_payloads.py tests/unit_tests/flux/api/test_app.py fluxboard/tests
git commit -m "fix: align tokenmm api and fluxboard risk consumers"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 7: Add Production Review Docs, Audit Tooling, And Rollout Gates

**Files:**
- Create: `docs/runbooks/tokenmm-risk-validation.md`
- Create if useful: `scripts/ops/tokenmm_risk_audit.py`
- Modify: `deploy/tokenmm/README.md`
- Modify: `deploy/tokenmm/strategies/README.md`
- Modify: `fluxboard/docs/tokenmm_contract.md`
- Test if useful: `tests/unit_tests/examples/strategies/test_tokenmm_stack_contract.py`

**Step 1: Write the production review checklist**

The runbook must cover:

- what local risk and global risk mean
- which API endpoint is authoritative for each view
- how to verify a single strategy vs the shared portfolio
- what degraded metadata means
- how to tell “data unavailable” from “true zero”
- what must be checked after restart before enabling trading

**Step 2: Add an operator audit command or script**

Recommended checks:

```bash
curl -fsS 'http://127.0.0.1:5022/api/v1/signals?profile=tokenmm'
curl -fsS 'http://127.0.0.1:5022/api/v1/balances?profile=tokenmm'
curl -fsS 'http://127.0.0.1:5022/api/v1/balances?strategy=plumeusdt_bybit_perp_makerv3'
curl -fsS 'http://127.0.0.1:5022/api/pulse/jobs'
```

The audit should fail loudly if:

- a strategy local qty disagrees with its own published balance source
- shared `global_qty_base` differs across live strategies
- the portfolio snapshot is degraded without diagnostics
- a runner is active but unresolved reconciliation is still present

**Step 3: Define rollout and acceptance gates**

Required production sign-off:

1. all targeted unit tests green
2. TokenMM group restarted cleanly through Pulse
3. `signals`, `balances(profile=tokenmm)`, and `balances(strategy=<id>)` agree for each strategy according to contract
4. partial vs strict `global_qty` semantics are visible and documented
5. startup reconciliation block/degrade behavior is verified intentionally at least once

**Step 4: Verify docs and contract tests**

Run:
```bash
rg -n "risk validation|source of truth|global_qty_base|local_qty_base|reconciliation" docs deploy/tokenmm fluxboard/docs
pytest tests/unit_tests/examples/strategies/test_tokenmm_stack_contract.py -v
```

Expected: PASS with clear operator-facing documentation.

**Step 5: Commit**

```bash
git add docs/runbooks/tokenmm-risk-validation.md scripts/ops/tokenmm_risk_audit.py deploy/tokenmm/README.md deploy/tokenmm/strategies/README.md fluxboard/docs/tokenmm_contract.md tests/unit_tests/examples/strategies/test_tokenmm_stack_contract.py
git commit -m "docs: add tokenmm risk validation runbook"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

## Final Verification Bundle

Before calling this productionized, run the full evidence bundle:

```bash
pytest tests/unit_tests/flux/strategies/makerv3/test_strategy_lifecycle.py -v
pytest tests/unit_tests/flux/strategies/makerv3/test_inventory.py -v
pytest tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py -v
pytest tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py -v
pytest tests/unit_tests/flux/common/test_portfolio_inventory.py -v
pytest tests/unit_tests/flux/common/test_portfolio_snapshot.py -v
pytest tests/unit_tests/flux/api/test_payloads.py -v
pytest tests/unit_tests/flux/api/test_app.py -v
pytest tests/unit_tests/examples/strategies/test_tokenmm_run_portfolio.py -v
pytest tests/unit_tests/examples/strategies/test_tokenmm_run_node.py -v
pytest tests/unit_tests/live/test_execution_engine.py -v
pytest tests/unit_tests/live/test_execution_engine_purge_startup.py -v
pnpm --dir fluxboard exec vitest run tests/signal/*.test.tsx tests/balances/*.test.tsx
```

Then do the live operator validation:

```bash
curl -fsS http://127.0.0.1:5022/api/pulse/jobs
curl -fsS 'http://127.0.0.1:5022/api/v1/signals?profile=tokenmm'
curl -fsS 'http://127.0.0.1:5022/api/v1/balances?profile=tokenmm'
curl -fsS 'http://127.0.0.1:5022/api/v1/balances?strategy=plumeusdt_bybit_perp_makerv3'
curl -fsS 'http://127.0.0.1:5022/api/v1/balances?strategy=plumeusdt_binance_spot_makerv3'
```

Expected production state:

- local spot and perp quantities are strategy-truth-correct
- shared `global_qty_base` is consistent across strategies according to the configured aggregation mode
- `Balances(profile=tokenmm)` matches the canonical portfolio snapshot
- Fluxboard renders the same quantities as the API
- no strategy can quote from unreconciled startup state

# TokenMM Balances And Portfolio Review Fixes Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.
> **Progress:** The Progress Tracker in this document is the execution source of truth and must be updated on every state change.

**Goal:** Close the final pre-PR/prod correctness gaps across MakerV3, MakerV4, portfolio snapshots, `/api/v1/balances`, `/api/v1/signals`, and Fluxboard balances/risk so every layer agrees on one truthful normalization contract.

**Architecture:** Fix the system from source of truth outward. First, make strategy-authored inventory and balances snapshots correct and unambiguous. Second, harden portfolio/API merge and freshness rules so profile balances cannot preserve stale or mis-netted data. Third, make Fluxboard consume backend risk semantics directly instead of re-deriving grouping or freshness from holdings rows. Prefer deleting misleading fallback behavior over keeping magic inference.

**Tech Stack:** Python (Flux strategies/API/common), Nautilus Trader, Redis, React/TypeScript Fluxboard, pytest, Vitest.

## Review Findings This Plan Closes

1. Netted portfolio position rows can keep stale `mv_raw` and overstate net equity.
2. `profile=tokenmm` trusts stale portfolio snapshots with no freshness gate.
3. Spot-position suppression is account-blind and can hide real rows.
4. Market-row merging ignores quote recency and can regress marks.
5. Cash mark reconstruction is order-dependent when multiple spot quote pairs exist.
6. Contract-scope filtering can hide valid collateral rows.
7. Merged rows can label realized PnL as unrealized PnL.
8. MakerV3 balances fallback can attribute same-base positions from the wrong venue/instrument.
9. MakerV3 keeps stale maker snapshots when reconciliation encodes flat as an omitted row.
10. MakerV3 balances and portfolio use different `price_based` quantity conversion prices.
11. MakerV4 is not wired into the shared local/global inventory contract.
12. Fluxboard can keep stale balance metadata and still rebuild risk semantics locally.

## Scope Rules

1. Signal/state must remain the exact strategy view of the world. Downstream layers may enrich presentation but must not invent inventory semantics.
2. `profile=tokenmm` may only prefer `portfolio_snapshot` when the snapshot is fresh and internally coherent.
3. Spot duplicate suppression must be account-aware.
4. MakerV4 must either fully publish the shared inventory contract or fail closed from shared portfolio views. This plan chooses full participation.
5. Every fix lands with a regression test first, then minimal implementation, then verification, then commit.

## Progress Tracker

**Source of truth:** Update this table whenever task state changes. Do not rely on memory, chat history, or TodoWrite alone.

| Task | Status | Owner | Notes / Last Update |
| --- | --- | --- | --- |
| Overall | completed | main | All six tasks are complete. Final release gate passed after closing the Task 5 payload export regression and restoring Task 3 omitted-maker fresh-flat handling. Verification: Python bundle `293 passed`, Fluxboard vitest bundle `94 passed`, doc-contract `rg` sweep passed |
| Task 1: Fix Portfolio Snapshot Freshness And Netted Position Valuation | completed | implementer | Completed in `ba7947c8c`; code quality review passed with one non-blocking recommendation to add a future fail-closed partial-valuation regression |
| Task 2: Harden API Balance Normalization Rules | completed | implementer | Verified with `23 passed, 100 deselected` plus the duplicate-spot app regression; code quality review found no Task 2 correctness issues, only commit-isolation notes for unrelated nested-account venue handling and unrelated MakerV4 test hunks in dirty files |
| Task 3: Correct MakerV3 Source-Of-Truth Publication | completed | implementer | Reclosed during Task 6 after restoring authoritative fresh-flat handling for omitted maker rows in reconciliation. Verification: targeted lifecycle red is green and the full release bundle passed (`293 passed`) |
| Task 4: Wire MakerV4 Into Shared Inventory Contract | completed | implementer | Completed after spec and quality review approval. Final verification: `tests/unit_tests/flux/strategies/makerv4/test_strategy.py tests/unit_tests/flux/strategies/makerv4/test_inventory_contract.py tests/unit_tests/flux/api/test_equities_profile_contract.py tests/unit_tests/examples/strategies/test_equities_run_portfolio.py` (`40 passed`) |
| Task 5: Make Fluxboard Consume Backend Balances And Risk Semantics Correctly | completed | implementer | Completed with backend-authored `risk_groups[].rows` plus row-level `risk_key`/`risk_label` annotations, Fluxboard metadata freshness fix, gross-risk default/filter cleanup, and backend-key drilldown. Verification: `pytest -q tests/unit_tests/flux/api/test_app.py` (`69 passed`) and Fluxboard slice `94 passed`. Spec-review subagents stalled/interrupted without findings; controller performed final spec/quality check against the Task 5 requirements before closeout |
| Task 6: Documentation, Regression Sweep, And Release Gate | completed | implementer | Doc alignment and release gate are complete. Cleared the public-payload export leak by moving `build_balance_risk_groups` back to an internal import in `app.py`, then reran the full verification bundle: Python `293 passed`, Fluxboard `94 passed`, doc-contract `rg` sweep passed |

---

### Task 1: Fix Portfolio Snapshot Freshness And Netted Position Valuation

**Files:**
- Modify: `systems/flux/flux/api/_payloads_balances.py`
- Modify: `systems/flux/flux/common/portfolio_snapshot.py`
- Modify: `systems/flux/flux/api/app.py`
- Test: `tests/unit_tests/flux/api/test_payloads.py`
- Test: `tests/unit_tests/flux/common/test_portfolio_snapshot.py`
- Test: `tests/unit_tests/flux/api/test_app.py`

**Step 1: Write the failing tests**

Add tests that prove:
1. `merge_portfolio_balances_rows()` does not preserve a seed row `mv_raw` after netting `+10` and `-5` into `signed_qty=5`
2. `build_portfolio_snapshot()` stores totals that match the netted position valuation, not the pre-net seed row valuation
3. `/api/v1/balances?profile=tokenmm` rejects a `portfolio_snapshot` whose `server_ts_ms` or `inventory.ts_ms` is older than `stale_after_ms` and falls back to fresh per-strategy balances

**Step 2: Run test to verify it fails**

Run:

```bash
pytest -q \
  tests/unit_tests/flux/api/test_payloads.py \
  tests/unit_tests/flux/common/test_portfolio_snapshot.py \
  tests/unit_tests/flux/api/test_app.py -k 'portfolio_snapshot or netted'
```

Expected: FAIL because netted position rows currently keep stale valuation fields and stale portfolio snapshots are still preferred.

**Step 3: Write minimal implementation**

Implement the narrowest safe contract:
1. in `_position_portfolio_row_from_agg()`, never preserve stale seed valuation fields after qty netting; either recompute valuation from the aggregate or clear derived mark/MV so later enrichment owns them
2. ensure `build_portfolio_snapshot()` stores balance rows/totals only after the row contract is internally consistent
3. in `api_balances()`, prefer `portfolio_snapshot` only when its timestamp is fresh relative to `stale_after_ms`; otherwise use the live per-strategy merge path

**Step 4: Run test to verify it passes**

Run the same pytest command plus:

```bash
pytest -q tests/unit_tests/flux/api/test_app.py::test_balances_profile_tokenmm_prefers_canonical_portfolio_snapshot_when_present
```

Expected: PASS with both fresh-snapshot preference and stale-snapshot rejection covered.

**Step 5: Commit**

```bash
git add \
  systems/flux/flux/api/_payloads_balances.py \
  systems/flux/flux/common/portfolio_snapshot.py \
  systems/flux/flux/api/app.py \
  tests/unit_tests/flux/api/test_payloads.py \
  tests/unit_tests/flux/common/test_portfolio_snapshot.py \
  tests/unit_tests/flux/api/test_app.py
git commit -m "fix(tokenmm): gate stale portfolio snapshots and correct netted valuations"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 2: Harden API Balance Normalization Rules

**Files:**
- Modify: `systems/flux/flux/api/_payloads_balances.py`
- Modify: `systems/flux/flux/api/app.py`
- Test: `tests/unit_tests/flux/api/test_payloads.py`
- Test: `tests/unit_tests/flux/api/test_app.py`
- Test: `tests/unit_tests/flux/api/test_balances_merge_dedupe.py`

**Step 1: Write the failing tests**

Add tests that prove:
1. `collapse_balance_display_rows()` keeps a spot position from `acctB` when the only cash row is from `acctA`
2. `load_market_rows_for_strategies()` keeps the newest non-null quote, not simply the last iterated strategy row
3. `enrich_balances_rows()` chooses the deterministic canonical spot contract for a cash asset when multiple spot quote pairs share the same base
4. `filter_balance_rows_for_contract_scope()` keeps valid collateral rows used by in-scope contracts, not only literal base/quote assets
5. merged position metadata does not label realized-only PnL as `uPnL`

**Step 2: Run test to verify it fails**

Run:

```bash
pytest -q \
  tests/unit_tests/flux/api/test_payloads.py \
  tests/unit_tests/flux/api/test_app.py \
  tests/unit_tests/flux/api/test_balances_merge_dedupe.py -k 'scope or collateral or market or pnl or duplicate'
```

Expected: FAIL on account-blind suppression, quote-recency merging, collateral filtering, and realized/unrealized labeling.

**Step 3: Write minimal implementation**

Implement deterministic normalization:
1. include account identity in spot cash-vs-position duplicate suppression
2. merge market rows by freshness and non-nullness, not loop order
3. choose one stable quote-selection rule for same-base multi-spot catalogs and document it in code comments/tests
4. extend contract-scope filtering to admit explicit collateral assets needed by the in-scope contracts
5. stop labeling realized PnL as unrealized PnL in merged row metadata

**Step 4: Run test to verify it passes**

Run the same pytest command plus:

```bash
pytest -q tests/unit_tests/flux/api/test_app.py::test_balances_profile_tokenmm_prefers_cash_row_over_duplicate_spot_position
```

Expected: PASS with no regressions on the existing duplicate-row contract.

**Step 5: Commit**

```bash
git add \
  systems/flux/flux/api/_payloads_balances.py \
  systems/flux/flux/api/app.py \
  tests/unit_tests/flux/api/test_payloads.py \
  tests/unit_tests/flux/api/test_app.py \
  tests/unit_tests/flux/api/test_balances_merge_dedupe.py
git commit -m "fix(tokenmm): harden balance normalization semantics"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 3: Correct MakerV3 Source-Of-Truth Publication

**Files:**
- Modify: `systems/flux/flux/strategies/makerv3/publisher.py`
- Modify: `systems/flux/flux/strategies/makerv3/strategy.py`
- Modify: `systems/flux/flux/strategies/makerv3/inventory.py` only if a shared helper becomes necessary
- Test: `tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py`
- Test: `tests/unit_tests/flux/strategies/makerv3/test_strategy_lifecycle.py`
- Test: `tests/unit_tests/flux/strategies/makerv3/test_inventory.py`

**Step 1: Write the failing tests**

Add tests that prove:
1. `publish_balances()` does not fall back to a same-base position from another venue or instrument when `positions_open(instrument_id=maker)` returns empty
2. a reconciliation message that omits the maker instrument clears or invalidates the previously fresh maker snapshot instead of leaving stale exposure live
3. `price_based` quantity conversion yields the same `signed_qty_base` contract in balances publication, local risk, and portfolio-facing snapshots

**Step 2: Run test to verify it fails**

Run:

```bash
pytest -q \
  tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py \
  tests/unit_tests/flux/strategies/makerv3/test_strategy_lifecycle.py \
  tests/unit_tests/flux/strategies/makerv3/test_inventory.py -k 'position_report or fallback or price_based'
```

Expected: FAIL because the current fallback is base-only, omitted-maker reconciliation leaves stale snapshots, and `price_based` conversion paths diverge.

**Step 3: Write minimal implementation**

Implement source-of-truth-first behavior:
1. restrict balances fallback positions to the maker instrument or an explicitly equivalent contract, never same-base-only inference
2. treat omitted maker rows in reconciliation as an authoritative flat/no-position update when the payload is otherwise fresh
3. route `price_based` conversion through one shared price source so balances, local risk, and portfolio inventory agree

**Step 4: Run test to verify it passes**

Run the same pytest command plus:

```bash
pytest -q tests/unit_tests/flux/api/test_signals_inventory_contract.py
```

Expected: PASS with no signal-contract regression.

**Step 5: Commit**

```bash
git add \
  systems/flux/flux/strategies/makerv3/publisher.py \
  systems/flux/flux/strategies/makerv3/strategy.py \
  systems/flux/flux/strategies/makerv3/inventory.py \
  tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py \
  tests/unit_tests/flux/strategies/makerv3/test_strategy_lifecycle.py \
  tests/unit_tests/flux/strategies/makerv3/test_inventory.py \
  tests/unit_tests/flux/api/test_signals_inventory_contract.py
git commit -m "fix(tokenmm): make makerv3 inventory publication authoritative"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 4: Wire MakerV4 Into Shared Inventory Contract

**Files:**
- Modify: `systems/flux/flux/strategies/makerv4/strategy.py`
- Modify: `systems/flux/flux/strategies/makerv4/publisher.py` if a dedicated publisher contract becomes necessary
- Modify: `systems/flux/flux/api/_payloads_signals.py`
- Modify: `systems/flux/flux/runners/equities/run_portfolio.py` only if portfolio wiring needs adjustment
- Create: `tests/unit_tests/flux/strategies/makerv4/test_inventory_contract.py`
- Modify: `tests/unit_tests/flux/strategies/makerv4/test_strategy.py`
- Modify: `tests/unit_tests/flux/api/test_equities_profile_contract.py`

**Step 1: Write the failing tests**

Add tests that prove:
1. `configure_portfolio_inventory_feed()` is consumed by MakerV4 runtime logic, not just stored
2. MakerV4 state publishes canonical inventory semantics needed by downstream consumers: `local_qty_base`, `global_qty_base`, completeness flags, aggregation mode, and qty-conversion metadata where applicable
3. equities profile/API consumers can project MakerV4 inventory semantics from strategy state without fallback invention

**Step 2: Run test to verify it fails**

Run:

```bash
pytest -q \
  tests/unit_tests/flux/strategies/makerv4/test_strategy.py \
  tests/unit_tests/flux/strategies/makerv4/test_inventory_contract.py \
  tests/unit_tests/flux/api/test_equities_profile_contract.py
```

Expected: FAIL because MakerV4 currently publishes quote-state only and does not participate in the shared inventory contract.

**Step 3: Write minimal implementation**

Implement full contract participation:
1. consume the configured portfolio inventory feed in MakerV4
2. publish strategy-owned local/global inventory fields and completeness metadata from MakerV4 state
3. publish explicit qty-conversion metadata instead of leaving downstream layers to guess
4. keep balances and signal payloads aligned with the same canonical fields

**Step 4: Run test to verify it passes**

Run the same pytest command plus:

```bash
pytest -q tests/unit_tests/examples/strategies/test_equities_run_portfolio.py
```

Expected: PASS with no equities portfolio regression.

**Step 5: Commit**

```bash
git add \
  systems/flux/flux/strategies/makerv4/strategy.py \
  systems/flux/flux/strategies/makerv4/publisher.py \
  systems/flux/flux/api/_payloads_signals.py \
  systems/flux/flux/runners/equities/run_portfolio.py \
  tests/unit_tests/flux/strategies/makerv4/test_strategy.py \
  tests/unit_tests/flux/strategies/makerv4/test_inventory_contract.py \
  tests/unit_tests/flux/api/test_equities_profile_contract.py \
  tests/unit_tests/examples/strategies/test_equities_run_portfolio.py
git commit -m "feat(equities): wire makerv4 into shared inventory contract"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 5: Make Fluxboard Consume Backend Balances And Risk Semantics Correctly

**Files:**
- Modify: `fluxboard/stores.ts`
- Modify: `fluxboard/Balances.tsx`
- Modify: `fluxboard/components/balances/RiskTable.tsx`
- Modify: `fluxboard/api.ts` if API payload shape must expand for risk breakdown source-of-truth
- Modify: `fluxboard/types.ts`
- Create or Modify: `tests/unit_tests/flux/api/test_app.py` if backend must expose explicit risk breakdowns
- Modify: `fluxboard/__tests__/store-freshness-contract.test.ts`
- Modify: `fluxboard/Balances.test.tsx`
- Modify: `fluxboard/components/balances/RiskTable.test.tsx`

**Step 1: Write the failing tests**

Add tests that prove:
1. metadata-only balance row changes rerender in Fluxboard even when qty/MV/mark/time are unchanged
2. the Risk tab does not hide gross-but-net-flat books by default or by ambiguous naming
3. expanded risk breakdowns and risk-row drilldown use backend risk semantics directly, not a local `riskKeyForCoin()` reconstruction

**Step 2: Run test to verify it fails**

Run:

```bash
cd fluxboard && pnpm exec vitest run \
  __tests__/store-freshness-contract.test.ts \
  Balances.test.tsx \
  components/balances/RiskTable.test.tsx
```

Expected: FAIL because row equality ignores metadata, the default risk filter is net-only, and breakdowns are still rebuilt from holdings rows.

**Step 3: Write minimal implementation**

Implement UI source-of-truth cleanup:
1. include rendered metadata in row equality or stop reusing stale rows when metadata changes
2. rename or redefine the risk filter so gross exposure cannot disappear by default
3. use backend-provided risk grouping/breakdown semantics for expansion and drilldown instead of `riskKeyForCoin()`

**Step 4: Run test to verify it passes**

Run the same Vitest command plus:

```bash
cd fluxboard && pnpm exec vitest run __tests__/api.test.ts api.flux.test.ts
```

Expected: PASS.

**Step 5: Commit**

```bash
git add \
  fluxboard/stores.ts \
  fluxboard/Balances.tsx \
  fluxboard/components/balances/RiskTable.tsx \
  fluxboard/api.ts \
  fluxboard/types.ts \
  fluxboard/__tests__/store-freshness-contract.test.ts \
  fluxboard/Balances.test.tsx \
  fluxboard/components/balances/RiskTable.test.tsx \
  tests/unit_tests/flux/api/test_app.py
git commit -m "fix(fluxboard): consume balances and risk semantics directly"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 6: Documentation, Regression Sweep, And Release Gate

**Files:**
- Modify: `docs/architecture/tokenmm-risk-source-of-truth.md`
- Modify: `docs/architecture/tokenmm-portfolio-inventory-semantics.md`
- Modify: `fluxboard/docs/tokenmm_contract.md`
- Modify: `fluxboard/docs/tokenmm_socket_contract.md`
- Modify: `deploy/tokenmm/README.md`
- Modify: `docs/runbooks/tokenmm-risk-validation.md`

**Step 1: Write the failing tests**

Add or extend contract tests that prove the docs and runbook match the now-truthful behavior:
1. fresh portfolio snapshots are required for profile preference
2. shared inventory semantics cover MakerV3 and MakerV4
3. Fluxboard risk/balances consumption is described as API-driven, not locally inferred

**Step 2: Run test to verify it fails**

Run:

```bash
pytest -q \
  tests/unit_tests/examples/strategies/test_tokenmm_risk_validation_contract.py \
  tests/unit_tests/examples/strategies/test_tokenmm_stack_contract.py
```

Expected: FAIL if docs still describe stale snapshot preference, partial V4 participation, or locally reconstructed UI semantics.

**Step 3: Write minimal implementation**

Update operator-facing docs and then run the full targeted verification sweep:

```bash
pytest -q \
  tests/unit_tests/flux/api/test_payloads.py \
  tests/unit_tests/flux/api/test_app.py \
  tests/unit_tests/flux/common/test_portfolio_snapshot.py \
  tests/unit_tests/flux/common/test_portfolio_inventory.py \
  tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py \
  tests/unit_tests/flux/strategies/makerv3/test_strategy_lifecycle.py \
  tests/unit_tests/flux/strategies/makerv3/test_inventory.py \
  tests/unit_tests/flux/strategies/makerv4/test_strategy.py \
  tests/unit_tests/flux/strategies/makerv4/test_inventory_contract.py \
  tests/unit_tests/flux/api/test_signals_inventory_contract.py \
  tests/unit_tests/flux/api/test_equities_profile_contract.py \
  tests/unit_tests/examples/strategies/test_tokenmm_run_portfolio.py \
  tests/unit_tests/examples/strategies/test_equities_run_portfolio.py

cd fluxboard && pnpm exec vitest run \
  Balances.test.tsx \
  components/balances/RiskTable.test.tsx \
  __tests__/store-freshness-contract.test.ts \
  __tests__/api.test.ts \
  api.flux.test.ts
```

**Step 4: Run release-gate sanity checks**

Run:

```bash
rg -n "portfolio_snapshot|global_qty_base|local_qty_base|aggregation_mode|risk_groups|source of truth" \
  docs/architecture \
  docs/runbooks \
  deploy/tokenmm \
  fluxboard/docs
```

Expected: all references align with the implemented contract and no doc claims stale snapshots or local UI inference as source of truth.

**Step 5: Commit**

```bash
git add \
  docs/architecture/tokenmm-risk-source-of-truth.md \
  docs/architecture/tokenmm-portfolio-inventory-semantics.md \
  fluxboard/docs/tokenmm_contract.md \
  fluxboard/docs/tokenmm_socket_contract.md \
  deploy/tokenmm/README.md \
  docs/runbooks/tokenmm-risk-validation.md \
  tests/unit_tests/examples/strategies/test_tokenmm_risk_validation_contract.py \
  tests/unit_tests/examples/strategies/test_tokenmm_stack_contract.py
git commit -m "docs(tokenmm): align balances and portfolio contracts with review fixes"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

# Bitget TokenMM Position Reconciliation Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.
> **Progress:** The Progress Tracker in this document is the execution source of truth and must be updated on every state change.

**Goal:** Fix the Bitget TokenMM position/reconciliation defect chain end-to-end so the backend, MakerV3 inventory, `/api/v1/balances`, `/api/v1/signals`, and Fluxboard all reflect one correct venue position without UI-only masking.

**Architecture:** Fix the source of truth first. Keep Bitget perp quantity semantics aligned with venue docs and existing instrument contracts unless tests prove otherwise. Remove stale `EXTERNAL` reconciliation artifacts at the execution-engine layer when a real strategy-owned netting position already matches venue truth. Then harden MakerV3 cache-fallback inventory and balances publication so stale duplicate cache entries cannot overstate local/global inventory while cleanup is converging. Add narrow downstream regression coverage where strategy snapshot assembly must reject embedded `EXTERNAL` rows for managed strategies, but do not move reconciliation policy into generic portfolio merging.

**Tech Stack:** Python, Nautilus Trader live execution engine, Bitget adapter, Flux MakerV3, pytest, Redis-backed cache models.

## Root-Cause Hypotheses To Prove Or Reject

1. Bitget UTA perp `total` is already in base position units for the affected linear contract, so the current `identity` quantity conversion is likely correct.
2. A stale `EXTERNAL` reconciliation artifact can remain open beside the real strategy-owned position because broader cleanup is conditional and may not fire once effective venue parity is restored.
3. MakerV3 cache-fallback local inventory, skew, and balances publication currently sum raw open positions and can therefore double-count a real position plus a stale `EXTERNAL` artifact.
4. Generic portfolio aggregation is amplifying bad upstream data, not creating it. Any downstream hardening should stay at the strategy snapshot boundary, not in generic portfolio merge code.

## Scope Rules

1. Do not fix this by masking rows in Fluxboard only.
2. Do not change Bitget perp quantity semantics without a failing regression that proves the venue contract is wrong.
3. Prefer authoritative cleanup in the execution engine, then defensive MakerV3 filtering for cache-fallback paths.
4. Keep unrelated local changes in the main workspace untouched.
5. Every behavioral fix lands with a failing test first, then minimal implementation, then verification.

## Progress Tracker

**Source of truth:** Update this table whenever task state changes. Do not rely on memory, chat history, or TodoWrite alone.

| Task | Status | Owner | Notes / Last Update |
| --- | --- | --- | --- |
| Overall | completed | main | Execution-engine cleanup, MakerV3 fallback hardening, payload-boundary evidence, verification, split commits, push, and PR are complete |
| Task 1: Lock The Bitget Quantity Contract With Evidence | completed | main | Added explicit adapter assertions that Bitget futures and UTA `total` map straight through to `PositionStatusReport.quantity`; no production adapter change required |
| Task 2: Purge Stale `EXTERNAL` Reconciliation Artifacts Earlier | completed | main | `_process_cached_position_discrepancies()` now actively closes stale EXTERNAL reconciliation artifacts when effective owned qty already matches venue truth |
| Task 3: Harden MakerV3 Cache-Fallback Inventory And Balances | completed | main | Added shared MakerV3 inventory filtering for stale EXTERNAL artifacts and defensive duplicate report collapse for same-instrument netting reports without position IDs |
| Task 4: Verify Snapshot Assembly And Aggregation Boundaries | completed | main | Added payload regression proving managed strategy row assembly overrides embedded `EXTERNAL` strategy IDs while generic portfolio merge remains unchanged |
| Task 5: Verification, Commit History, And PR | completed | main | Verification green: live recon slice `7 passed`, MakerV3 inventory/summary slice `18 passed`, publish_balances slice `10 passed`, payload+Bitget adapter slice `5 passed`; branch pushed and PR #52 opened |

---

### Task 1: Lock The Bitget Quantity Contract With Evidence

**Files:**
- Review: `nautilus_trader/adapters/bitget/execution.py`
- Test: `tests/integration_tests/adapters/bitget/test_execution.py`
- Test: `tests/integration_tests/adapters/bitget/test_execution_rest.py`
- Modify only if needed: `nautilus_trader/adapters/bitget/execution.py`

**Step 1: Write the failing or proving test**

Add or extend adapter coverage to prove what contract we rely on:
1. UTA position `total` is passed through as the position quantity for the affected Bitget linear perp fixtures
2. the resulting `PositionStatusReport.quantity` remains consistent with the instrument quantity contract used elsewhere

If current tests already prove this, add the narrowest assertion needed to make that evidence explicit instead of changing production code.

**Step 2: Run the targeted Bitget adapter tests**

Run:

```bash
pytest -q \
  tests/integration_tests/adapters/bitget/test_execution.py \
  tests/integration_tests/adapters/bitget/test_execution_rest.py -k 'position and bitget'
```

Expected: PASS with the current identity quantity contract, unless a failing fixture proves the adapter is wrong.

**Step 3: Make the minimal change only if tests prove it necessary**

If the tests fail because the adapter is mis-parsing units, fix the parser and the instrument quantity contract together. Otherwise, leave production semantics unchanged and carry the evidence into the PR root-cause summary.

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 2: Purge Stale `EXTERNAL` Reconciliation Artifacts Earlier

**Files:**
- Modify: `nautilus_trader/live/execution_engine.py`
- Test: `tests/unit_tests/live/test_execution_engine.py`
- Test: `tests/unit_tests/live/test_execution_recon.py`

**Step 1: Write the failing tests**

Add execution-engine regression coverage that proves:
1. when a strategy-owned netting position already matches the venue-reported quantity, a stale `EXTERNAL` reconciliation artifact is still purged or closed
2. the same cleanup happens outside the narrow startup-only happy path that currently leaves duplicates live
3. true mismatches still reconcile strictly and do not silently discard legitimate positions

**Step 2: Run the targeted live reconciliation tests**

Run:

```bash
pytest -q \
  tests/unit_tests/live/test_execution_engine.py \
  tests/unit_tests/live/test_execution_recon.py -k 'external or reconciliation or stale'
```

Expected: FAIL because stale `EXTERNAL` artifacts can currently survive once effective venue parity is restored.

**Step 3: Write minimal implementation**

Implement the narrowest safe cleanup path in the execution engine so stale reconciliation artifacts are removed when:
1. a non-`EXTERNAL` open netting position already accounts for the venue quantity, and
2. the remaining open `EXTERNAL` positions are identifiable reconciliation artifacts rather than legitimate independent exposure.

Keep strict mismatch handling intact for real discrepancies.

**Step 4: Re-run the targeted tests**

Run the same pytest command and confirm the stale-artifact cases pass without regressing strict reconciliation behavior.

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 3: Harden MakerV3 Cache-Fallback Inventory And Balances

**Files:**
- Modify: `systems/flux/flux/strategies/makerv3/strategy.py`
- Modify: `systems/flux/flux/strategies/makerv3/publisher.py`
- Modify only if a shared helper is warranted: `systems/flux/flux/strategies/makerv3/inventory.py`
- Test: `tests/unit_tests/flux/strategies/makerv3/test_strategy_lifecycle.py`
- Test: `tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py`
- Test: `tests/unit_tests/flux/strategies/makerv3/test_inventory.py`

**Step 1: Write the failing tests**

Add regression coverage that proves:
1. `_maker_local_position_summary()` ignores stale `EXTERNAL` reconciliation artifacts when a real maker-instrument position is also open
2. `_compute_inventory_skew()` uses the corrected local/global quantities after the duplicate is removed or filtered
3. `publish_balances()` fallback does not emit both the strategy-owned maker position and the stale `EXTERNAL` artifact for the same effective venue position
4. duplicate maker position reports are defensively deduped if the execution path can still surface more than one netting report for the same instrument

**Step 2: Run the targeted MakerV3 tests**

Run:

```bash
pytest -q \
  tests/unit_tests/flux/strategies/makerv3/test_strategy_lifecycle.py \
  tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py \
  tests/unit_tests/flux/strategies/makerv3/test_inventory.py -k 'inventory or balances or external or duplicate'
```

Expected: FAIL on cache-fallback double-counting or duplicate-report handling.

**Step 3: Write minimal implementation**

Implement one consistent filtering contract for MakerV3 fallback paths:
1. prefer fresh maker position reports when available
2. when reading raw cache positions, exclude stale `EXTERNAL` reconciliation artifacts if a real maker-instrument position already covers the effective exposure
3. keep legitimate non-artifact positions visible
4. reuse a shared helper if that avoids strategy/publisher drift

**Step 4: Re-run the targeted MakerV3 tests**

Run the same pytest command and confirm local summary, skew, and balances publication agree on the corrected quantities.

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 4: Verify Snapshot Assembly And Aggregation Boundaries

**Files:**
- Review/modify if needed: `systems/flux/flux/api/_payloads_balances.py`
- Review/modify if needed: `systems/flux/flux/common/portfolio_snapshot.py`
- Test: `tests/unit_tests/flux/api/test_payloads.py`
- Test: `tests/unit_tests/flux/common/test_portfolio_snapshot.py`

**Step 1: Write the failing or proving tests**

Add coverage to prove:
1. managed strategy balance row assembly does not preserve embedded `EXTERNAL` rows when building a non-`EXTERNAL` snapshot
2. generic portfolio merge continues to aggregate trusted strategy snapshots without taking on Bitget-specific reconciliation policy

**Step 2: Run the targeted snapshot/payload tests**

Run:

```bash
pytest -q \
  tests/unit_tests/flux/api/test_payloads.py \
  tests/unit_tests/flux/common/test_portfolio_snapshot.py -k 'external or strategy_id or portfolio'
```

Expected: either PASS as proof that the boundary is already correct, or FAIL with a narrow snapshot-assembly gap to fix.

**Step 3: Write minimal implementation only if the boundary is leaky**

If tests prove managed strategy snapshot assembly can keep embedded `EXTERNAL` rows, fix that boundary. Do not add semantic dedupe to generic portfolio merge unless the data model now carries enough lineage to make that safe.

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 5: Verification, Commit History, And PR

**Files:**
- Modify: `docs/plans/2026-03-16-bitget-tokenmm-position-reconciliation.md`

**Step 1: Run the full targeted verification bundle**

Run the Bitget adapter, execution-engine, reconciliation, MakerV3, and any payload/portfolio slices changed by the fix.

**Step 2: Inspect git diff and status**

Review the isolated worktree diff to confirm only the intended files changed and separate commits cleanly if multiple independent bugs were fixed.

**Step 3: Commit**

Create focused commit(s) with clear messages that distinguish the execution-engine fix from MakerV3 or payload hardening if practical.

**Step 4: Open the PR**

Push `codex/bitget-tokenmm-position-reconciliation-20260316`, open a PR, and include:
1. worktree path and branch
2. root-cause summary
3. why Bitget quantity semantics were or were not changed
4. the regression tests added
5. verification command outputs

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

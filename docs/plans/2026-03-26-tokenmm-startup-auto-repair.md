# TokenMM Startup Auto-Repair Implementation Plan

> **For the execution agent:** REQUIRED SUB-SKILL: Before implementing this plan, choose exactly one execution mode and use the matching skill: `superpowers:subagent-driven-development` for same-session execution or `superpowers:executing-plans` for a separate-session handoff.
> **Progress:** The Progress Tracker in this document is the execution source of truth and must be updated on every state change.

**Goal:** Implement evidence-gated startup auto-repair for TokenMM netting reconciliation so proven stale or double-applied local cache state is repaired automatically while ambiguous mismatches still fail closed.

**Architecture:** Lock the March 26 Binance spot and perp failures into focused unit regressions first, then harden [execution_engine.py](/home/ubuntu/nautilus_trader/nautilus_trader/live/execution_engine.py) in three passes: stale-order-plus-stale-position cleanup, bounded startup fill replay with idempotency, and bounded same-strategy orphan lineage restore. Finish by surfacing startup repair classifications explicitly and by updating the TokenMM readiness rollout config so Binance failures are no longer invisible in the production readiness profile.

**Tech Stack:** Python, Nautilus live execution engine, Flux TokenMM deployment config, pytest.

## Progress Tracker

**Source of truth:** Update this table whenever task state changes. Do not rely on memory, chat history, or TodoWrite alone.

| Task | Status | Owner | Depends On | Write Scope | Lane Branch | Worktree Path | Commit / Diff | Verification | Notes / Last Update |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Task 1: Auto-repair stale startup spot state when venue is already flat | not_started | unassigned | none | `nautilus_trader/live/execution_engine.py`, `tests/unit_tests/live/test_execution_recon.py` | `shared` | `shared` | `none` | `not_run` | Plan created |
| Task 2: Prevent startup perp fill replay from drifting local qty above venue truth | not_started | unassigned | Task 1: Auto-repair stale startup spot state when venue is already flat | `nautilus_trader/live/execution_engine.py`, `nautilus_trader/live/reconciliation.py`, `tests/unit_tests/live/test_execution_recon.py` | `shared` | `shared` | `none` | `not_run` | Plan created |
| Task 3: Restore bounded grouped orphan lineage without reopening ambiguous subset guessing | not_started | unassigned | Task 2: Prevent startup perp fill replay from drifting local qty above venue truth | `nautilus_trader/live/execution_engine.py`, `tests/unit_tests/live/test_execution_recon.py` | `shared` | `shared` | `none` | `not_run` | Plan created |
| Task 4: Emit explicit startup repair classifications and alerts | not_started | unassigned | Task 3: Restore bounded grouped orphan lineage without reopening ambiguous subset guessing | `nautilus_trader/live/execution_engine.py`, `tests/unit_tests/live/test_execution_recon.py` | `shared` | `shared` | `none` | `not_run` | Plan created |
| Task 5: Verify the focused suite and update rollout config and notes | not_started | unassigned | Task 1: Auto-repair stale startup spot state when venue is already flat, Task 2: Prevent startup perp fill replay from drifting local qty above venue truth, Task 3: Restore bounded grouped orphan lineage without reopening ambiguous subset guessing, Task 4: Emit explicit startup repair classifications and alerts | `deploy/tokenmm/tokenmm.live.toml`, `docs/plans/2026-03-26-tokenmm-startup-auto-repair.md` | `shared` | `shared` | `none` | `not_run` | Plan created |

---

### Task 1: Auto-repair stale startup spot state when venue is already flat

**Files:**
- Modify: `tests/unit_tests/live/test_execution_recon.py`
- Modify: `nautilus_trader/live/execution_engine.py`

**Dependencies:** `none`

**Write Scope:** `tests/unit_tests/live/test_execution_recon.py`, `nautilus_trader/live/execution_engine.py`

**Verification Commands:**
- `pytest -q tests/unit_tests/live/test_execution_recon.py -k "auto_repairs_stale_startup_position_with_recent_fills_binance_spot_shape"`
- `pytest -q tests/unit_tests/live/test_execution_recon.py -k "auto_repairs_stale_startup_position_with_recent_fills_binance_spot_shape or cleans_stale_startup_position_after_missing_targeted_open_order_query"`

**Step 1: Write the failing test**
- Add `test_reconcile_execution_state_auto_repairs_stale_startup_position_with_recent_fills_binance_spot_shape`.
- Model the live spot shape directly:
  - one cached net short position
  - one cached startup open order
  - venue position report is flat
  - bulk open-order sweep omits the cached open order
  - targeted open-order query returns `None`
  - startup fill reports are present in the same window
- Assert the current code fails startup or leaves the stale cached position in place.

**Step 2: Run test to verify it fails**
- Run: `pytest -q tests/unit_tests/live/test_execution_recon.py -k "auto_repairs_stale_startup_position_with_recent_fills_binance_spot_shape"`
- Expected: FAIL because startup still treats the stale cached open order as blocking evidence and does not close the stale cached position.

**Step 3: Write minimal implementation**
- In [execution_engine.py](/home/ubuntu/nautilus_trader/nautilus_trader/live/execution_engine.py), resolve startup open orders missing at venue before the stale-position cleanup guard makes its final decision.
- Recompute `current_startup_open_orders` from post-resolution cache state, not pre-resolution snapshot state.
- Keep the repair scoped to startup NETTING reconciliation only.

**Step 4: Run test to verify it passes**
- Run: `pytest -q tests/unit_tests/live/test_execution_recon.py -k "auto_repairs_stale_startup_position_with_recent_fills_binance_spot_shape or cleans_stale_startup_position_after_missing_targeted_open_order_query"`
- Expected: PASS, with the new spot-shape regression and the existing stale-startup cleanup regression both green.

**Step 5: Commit**
- `git add tests/unit_tests/live/test_execution_recon.py nautilus_trader/live/execution_engine.py`
- `git commit -m "fix(reconciliation): auto-repair stale startup spot cache state"`

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 2: Prevent startup perp fill replay from drifting local qty above venue truth

**Files:**
- Modify: `tests/unit_tests/live/test_execution_recon.py`
- Modify: `nautilus_trader/live/execution_engine.py`
- Modify if needed: `nautilus_trader/live/reconciliation.py`

**Dependencies:** `Task 1: Auto-repair stale startup spot state when venue is already flat`

**Write Scope:** `tests/unit_tests/live/test_execution_recon.py`, `nautilus_trader/live/execution_engine.py`, `nautilus_trader/live/reconciliation.py`

**Verification Commands:**
- `pytest -q tests/unit_tests/live/test_execution_recon.py -k "does_not_double_apply_startup_fill_when_cached_netting_qty_already_matches_venue_binance_perp_shape"`
- `pytest -q tests/unit_tests/live/test_execution_recon.py -k "does_not_double_apply_startup_fill_when_cached_netting_qty_already_matches_venue_binance_perp_shape or uses_open_only_with_targeted_open_order_queries_when_startup_positions_exist"`

**Step 1: Write the failing test**
- Add `test_reconcile_execution_state_does_not_double_apply_startup_fill_when_cached_netting_qty_already_matches_venue_binance_perp_shape`.
- Model the live perp shape directly:
  - cached owned qty plus cached EXTERNAL qty already equals the venue position report
  - startup runs in `open_only=True`
  - startup fill reports are present in the same lookback window
  - startup targeted order queries resolve recent open orders
- Assert the current code incorrectly drifts local qty above venue truth or fails reconciliation.

**Step 2: Run test to verify it fails**
- Run: `pytest -q tests/unit_tests/live/test_execution_recon.py -k "does_not_double_apply_startup_fill_when_cached_netting_qty_already_matches_venue_binance_perp_shape"`
- Expected: FAIL because the startup path still skips the fill-reconciliation path too coarsely and can replay a fill already represented in cached state.

**Step 3: Write minimal implementation**
- Replace the blanket `snapshot.has_open_positions` fill-adjustment skip with evidence-gated logic.
- Reuse the existing duplicate-fill protections:
  - `trade_id` dedupe
  - cached fill lookup
  - inferred-fill timestamp guard
  - fill application audit trail
- Allow startup fill replay only when the fill is not already represented locally.

**Step 4: Run test to verify it passes**
- Run: `pytest -q tests/unit_tests/live/test_execution_recon.py -k "does_not_double_apply_startup_fill_when_cached_netting_qty_already_matches_venue_binance_perp_shape or uses_open_only_with_targeted_open_order_queries_when_startup_positions_exist"`
- Expected: PASS, while the existing open-only startup contract still stays green.

**Step 5: Commit**
- `git add tests/unit_tests/live/test_execution_recon.py nautilus_trader/live/execution_engine.py nautilus_trader/live/reconciliation.py`
- `git commit -m "fix(reconciliation): stop startup fill replay from drifting venue-matched qty"`

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 3: Restore bounded grouped orphan lineage without reopening ambiguous subset guessing

**Files:**
- Modify: `tests/unit_tests/live/test_execution_recon.py`
- Modify: `nautilus_trader/live/execution_engine.py`

**Dependencies:** `Task 2: Prevent startup perp fill replay from drifting local qty above venue truth`

**Write Scope:** `tests/unit_tests/live/test_execution_recon.py`, `nautilus_trader/live/execution_engine.py`

**Verification Commands:**
- `pytest -q tests/unit_tests/live/test_execution_recon.py -k "startup_restores_grouped_orphan_lineage_exactly or startup_does_not_combine_multiple_orphan_positions"`
- `pytest -q tests/unit_tests/live/test_execution_recon.py -k "grouped_orphan_lineage"`

**Step 1: Write the failing tests**
- Add or tighten one exact Binance-style grouped-orphan success case and one ambiguous failure case.
- Keep the success case same-strategy and exact-qty.
- Keep the failure case multi-fragment ambiguous so startup must still fail closed.

**Step 2: Run tests to verify the current behavior is too narrow**
- Run: `pytest -q tests/unit_tests/live/test_execution_recon.py -k "startup_restores_grouped_orphan_lineage_exactly or startup_does_not_combine_multiple_orphan_positions"`
- Expected: FAIL on the exact grouped same-strategy restore path after the March 25 simplification, while the ambiguous failure guard remains informative.

**Step 3: Write minimal implementation**
- Reintroduce a bounded exact-subset search in [execution_engine.py](/home/ubuntu/nautilus_trader/nautilus_trader/live/execution_engine.py).
- Keep all of these guards:
  - same strategy only
  - same instrument only
  - exact qty only
  - bounded subset search size
  - ambiguous matches still fail

**Step 4: Run test to verify it passes**
- Run: `pytest -q tests/unit_tests/live/test_execution_recon.py -k "startup_restores_grouped_orphan_lineage_exactly or startup_does_not_combine_multiple_orphan_positions"`
- Expected: PASS, with exact grouped restore allowed again and ambiguous combinations still rejected.

**Step 5: Commit**
- `git add tests/unit_tests/live/test_execution_recon.py nautilus_trader/live/execution_engine.py`
- `git commit -m "fix(reconciliation): restore bounded startup orphan lineage search"`

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 4: Emit explicit startup repair classifications and alerts

**Files:**
- Modify: `nautilus_trader/live/execution_engine.py`
- Modify: `tests/unit_tests/live/test_execution_recon.py`

**Dependencies:** `Task 3: Restore bounded grouped orphan lineage without reopening ambiguous subset guessing`

**Write Scope:** `nautilus_trader/live/execution_engine.py`, `tests/unit_tests/live/test_execution_recon.py`

**Verification Commands:**
- `pytest -q tests/unit_tests/live/test_execution_recon.py -k "startup_position_reconciliation"`
- `pytest -q tests/unit_tests/live/test_execution_recon.py -k "auto_repairs_stale_startup_position_with_recent_fills_binance_spot_shape or does_not_double_apply_startup_fill_when_cached_netting_qty_already_matches_venue_binance_perp_shape or startup_restores_grouped_orphan_lineage_exactly"`

**Step 1: Write the failing tests**
- Add assertions around startup alert payloads or published repair metadata for:
  - stale cached positions removed
  - stale startup orders marked missing at venue
  - orphan lineage restored
  - ambiguous startup mismatch failed closed
- Keep the assertions scoped to existing startup alert publication hooks instead of inventing a new reporting surface.

**Step 2: Run tests to verify the current alert payloads are not specific enough**
- Run: `pytest -q tests/unit_tests/live/test_execution_recon.py -k "startup_position_reconciliation"`
- Expected: FAIL because current startup alerts do not classify the repair cause and action precisely enough for operators.

**Step 3: Write minimal implementation**
- Extend the startup alert payload in [execution_engine.py](/home/ubuntu/nautilus_trader/nautilus_trader/live/execution_engine.py) with explicit `cause` and `action` fields.
- Emit one compact per-instrument startup repair summary in logs after classification.
- Do not add a second alerting system; reuse the existing startup position reconciliation alert path.

**Step 4: Run test to verify it passes**
- Run: `pytest -q tests/unit_tests/live/test_execution_recon.py -k "startup_position_reconciliation"`
- Expected: PASS, while the focused spot/perp/orphan regressions remain green.

**Step 5: Commit**
- `git add nautilus_trader/live/execution_engine.py tests/unit_tests/live/test_execution_recon.py`
- `git commit -m "feat(reconciliation): classify startup auto-repair actions"`

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 5: Verify the focused suite and update rollout config and notes

**Files:**
- Modify: `deploy/tokenmm/tokenmm.live.toml`
- Modify: `docs/plans/2026-03-26-tokenmm-startup-auto-repair.md`

**Dependencies:** `Task 1: Auto-repair stale startup spot state when venue is already flat`, `Task 2: Prevent startup perp fill replay from drifting local qty above venue truth`, `Task 3: Restore bounded grouped orphan lineage without reopening ambiguous subset guessing`, `Task 4: Emit explicit startup repair classifications and alerts`

**Write Scope:** `deploy/tokenmm/tokenmm.live.toml`, `docs/plans/2026-03-26-tokenmm-startup-auto-repair.md`

**Verification Commands:**
- `pytest -q tests/unit_tests/live/test_execution_recon.py`
- `pytest -q tests/unit_tests/flux/runners/test_tokenmm_readiness.py`
- `curl -fsS http://127.0.0.1:5022/api/v1/readiness?profile=tokenmm`

**Step 1: Run the focused verification suite**
- Run the pytest commands above after Tasks 1-4 land.
- Record exact command output and pass/fail state in the Progress Tracker.

**Step 2: Update rollout config**
- Modify [tokenmm.live.toml](/home/ubuntu/nautilus_trader/deploy/tokenmm/tokenmm.live.toml) so `tokenmm_required_strategy_ids` includes:
  - `plumeusdt_binance_spot_makerv3`
  - `plumeusdt_binance_perp_makerv3`
- Keep the change limited to readiness contributors; do not rewrite unrelated TokenMM strategy lists.

**Step 3: Write rollout notes**
- Document:
  - expected startup repair actions
  - what still fails closed
  - the exact restart order for the two Binance services
  - the readiness expectation after config update
  - the follow-up long-term cache redesign item

**Step 4: Run post-config verification**
- Run: `curl -fsS http://127.0.0.1:5022/api/v1/readiness?profile=tokenmm`
- Expected after deployment: the TokenMM readiness payload includes the Binance strategies as required contributors and reports them accurately.

**Step 5: Commit**
- `git add deploy/tokenmm/tokenmm.live.toml docs/plans/2026-03-26-tokenmm-startup-auto-repair.md`
- `git commit -m "docs: finalize tokenmm startup auto-repair rollout"`

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

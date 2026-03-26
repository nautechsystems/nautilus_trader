# Trades Blotter Hardening Implementation Plan

> **For the execution agent:** REQUIRED SUB-SKILL: Before implementing this plan, choose exactly one execution mode and use the matching skill: `superpowers:subagent-driven-development` for same-session execution or `superpowers:executing-plans` for a separate-session handoff.
> **Progress:** The Progress Tracker in this document is the execution source of truth and must be updated on every state change.

**Goal:** Finish the PR72 trades architecture so the trades blotter is truthful, stable, and free of permanent TokenMM compatibility mode.

**Architecture:** Keep the PR72 standard realtime transport as the source of truth for canonical trades live view. Fix the unfinished boundary layers around resync completion, health-state semantics, and TokenMM legacy trade rows instead of building a new transport. Land the code in five passes: surface-aware resync ownership, truthful trades health, TokenMM quantity-contract completion, recovery dedupe and cutover support, and final verification plus doc cleanup.

**Tech Stack:** React, TypeScript, Zustand, Socket.IO, Flask/Redis API, Python, Vitest, pytest.

## Progress Tracker

**Source of truth:** Update this table whenever task state changes. Do not rely on memory, chat history, or TodoWrite alone.

| Task | Status | Owner | Depends On | Write Scope | Lane Branch | Worktree Path | Commit / Diff | Verification | Notes / Last Update |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Task 1: Make resync completion surface-aware | completed | implementer | none | `fluxboard/stores.ts`, `fluxboard/sockets.ts`, `fluxboard/Trades.tsx`, `fluxboard/config/uiProfiles.ts`, `fluxboard/__tests__/resync-contract.test.tsx`, `fluxboard/__tests__/TradesStore.test.ts` | `fix/trades-blotter-hardening-20260326` | `.worktrees/trades-blotter-hardening` | `bacd41fb5f` | `PASS: \`VITEST_FULL=1 pnpm --dir fluxboard exec vitest run __tests__/resync-contract.test.tsx __tests__/TradesStore.test.ts __tests__/ResyncPolling.test.tsx stores/orderViewStore.test.ts __tests__/hooks/useResyncStatus.test.ts\`` | Committed surface-aware resync ownership. Residual quality note is limited to hypothetical future pages that mount `Trades` and `OrderView` together; current router/profile surfaces do not do that today |
| Task 2: Make trades health truthful for canonical and non-canonical views | completed | implementer | Task 1: Make resync completion surface-aware | `fluxboard/Trades.tsx`, `fluxboard/lib/realtime/types.ts`, `fluxboard/__tests__/trades-status.test.tsx`, `fluxboard/Trades.test.tsx`, `fluxboard/__tests__/trades-integration.test.tsx` | `fix/trades-blotter-hardening-20260326` | `.worktrees/trades-blotter-hardening` | `1f0f8c62d2` | `PASS: \`VITEST_FULL=1 pnpm --dir fluxboard exec vitest run __tests__/trades-status.test.tsx Trades.test.tsx __tests__/trades-integration.test.tsx\`` | Committed truthful health-state split: fresh snapshot-only views stay `LIVE`, and canonical reconnects still progress through `OFFLINE` -> `RECOVERING` -> `LIVE`. Review-agent responses during this task were stale Task 1 output, so Task 2 closed on local spec/quality review plus green verification |
| Task 3: Complete TokenMM normalized quantity writes and compatibility gating | completed | implementer | Task 2: Make trades health truthful for canonical and non-canonical views | `systems/flux/flux/strategies/shared/trades.py`, `systems/flux/flux/api/_payloads_common.py`, `systems/flux/flux/api/app.py`, `systems/flux/flux/api/socketio.py`, `tests/unit_tests/flux/strategies/shared/test_trades.py`, `tests/unit_tests/flux/api/test_tokenmm_compat.py`, `tests/unit_tests/flux/api/test_socketio_tokenmm.py`, `tests/unit_tests/flux/api/test_realtime_contract.py` | `fix/trades-blotter-hardening-20260326` | `.worktrees/trades-blotter-hardening` | `3fe947af14` | `PASS: \`PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 /home/ubuntu/nautilus_trader/.worktrees/prod-lanes-exec-20260326/.venv/bin/pytest -q tests/unit_tests/flux/strategies/shared/test_trades.py tests/unit_tests/flux/api/test_tokenmm_compat.py\`; PASS: \`PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 /home/ubuntu/nautilus_trader/.worktrees/prod-lanes-exec-20260326/.venv/bin/pytest -q tests/unit_tests/flux/api/test_socketio_tokenmm.py tests/unit_tests/flux/api/test_realtime_contract.py -k "trade_gap or compatibility or trades"\`` | Spec review and quality review both passed. Final diff shares degraded-status semantics from `quantity_units`, restores direct legacy bootstrap coverage, and keeps explicit degraded qty rows out of TokenMM compatibility/reset mode |
| Task 4: Dedupe recovery churn and document TokenMM stream cutover | completed | implementer | Task 3: Complete TokenMM normalized quantity writes and compatibility gating | `fluxboard/Trades.tsx`, `fluxboard/sockets.ts`, `systems/flux/flux/api/socketio.py`, `fluxboard/__tests__/trades-integration.test.tsx`, `fluxboard/__tests__/realtime/compatibility-matrix.test.tsx`, `tests/unit_tests/flux/api/test_socketio_tokenmm.py`, `tests/unit_tests/flux/api/test_realtime_contract.py`, `docs/runbooks/tokenmm-trades-blotter-cutover.md` | `fix/trades-blotter-hardening-20260326` | `.worktrees/trades-blotter-hardening` | `0168fcc494` | `PASS: \`VITEST_FULL=1 pnpm --dir fluxboard exec vitest run sockets.test.ts __tests__/trades-integration.test.tsx __tests__/realtime/compatibility-matrix.test.tsx\`; PASS: \`PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 /home/ubuntu/nautilus_trader/.worktrees/prod-lanes-exec-20260326/.venv/bin/pytest -q tests/unit_tests/flux/api/test_socketio_tokenmm.py tests/unit_tests/flux/api/test_realtime_contract.py -k "trade_gap or recovery_required or trades or standard_only_trade_gap"\`` | Completed the low-level churn fixes, added the planned frontend reconnect and standard-contract regressions, documented the TokenMM Redis trade-stream cutover, and exposed explicit recovery copy in `Trades.tsx` so reconnects and snapshot refreshes stop collapsing into the same generic replay banner |
| Task 5: Verify focused suites and finalize trades rollout docs | not_started | unassigned | Task 1: Make resync completion surface-aware, Task 2: Make trades health truthful for canonical and non-canonical views, Task 3: Complete TokenMM normalized quantity writes and compatibility gating, Task 4: Dedupe recovery churn and document TokenMM stream cutover | `docs/plans/realtime-surfaces/trades-cutover.md`, `fluxboard/docs/tokenmm_socket_contract.md`, `docs/plans/2026-03-26-trades-blotter-hardening.md` | `shared` | `shared` | `none` | `not_run` | Plan created |

---

### Task 1: Make resync completion surface-aware

**Files:**
- Modify: `fluxboard/stores.ts`
- Modify if needed: `fluxboard/sockets.ts`
- Modify if needed: `fluxboard/Trades.tsx`
- Modify if needed: `fluxboard/config/uiProfiles.ts`
- Test: `fluxboard/__tests__/resync-contract.test.tsx`
- Test: `fluxboard/__tests__/TradesStore.test.ts`

**Dependencies:** `none`

**Write Scope:** `fluxboard/stores.ts`, `fluxboard/sockets.ts`, `fluxboard/Trades.tsx`, `fluxboard/config/uiProfiles.ts`, `fluxboard/__tests__/resync-contract.test.tsx`, `fluxboard/__tests__/TradesStore.test.ts`

**Verification Commands:**
- `VITEST_FULL=1 pnpm --dir fluxboard exec vitest run __tests__/resync-contract.test.tsx __tests__/TradesStore.test.ts`

**Step 1: Write the failing tests**
- Add a focused contract test proving that a trades reconnect epoch on TokenMM does not require `order-view` acknowledgement to clear.
- Add the same assertion for Equities.
- Keep one regression proving that a surface which truly mounts both consumers still requires both acknowledgements.

**Step 2: Run tests to verify they fail**
- Run: `VITEST_FULL=1 pnpm --dir fluxboard exec vitest run __tests__/resync-contract.test.tsx __tests__/TradesStore.test.ts`
- Expected: FAIL because the store still uses the global hard-coded `['trades', 'order-view']` acknowledgement set.

**Step 3: Write minimal implementation**
- In [stores.ts](/home/ubuntu/nautilus_trader/fluxboard/stores.ts), replace the global consumer set with a surface-aware or mounted-consumer-aware contract.
- Keep the current epoch model and stale-ack protections.
- Do not change seq-gap detection or trades-local apply rules in this task.

**Step 4: Run tests to verify they pass**
- Run: `VITEST_FULL=1 pnpm --dir fluxboard exec vitest run __tests__/resync-contract.test.tsx __tests__/TradesStore.test.ts`
- Expected: PASS, with TokenMM and Equities no longer blocked by non-existent `order-view` acknowledgements.

**Step 5: Commit**
- `git add fluxboard/stores.ts fluxboard/sockets.ts fluxboard/Trades.tsx fluxboard/config/uiProfiles.ts fluxboard/__tests__/resync-contract.test.tsx fluxboard/__tests__/TradesStore.test.ts`
- `git commit -m "fix(trades): make resync completion surface-aware"`

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 2: Make trades health truthful for canonical and non-canonical views

**Files:**
- Modify: `fluxboard/Trades.tsx`
- Modify if needed: `fluxboard/lib/realtime/types.ts`
- Test: `fluxboard/__tests__/trades-status.test.tsx`
- Test: `fluxboard/Trades.test.tsx`
- Test: `fluxboard/__tests__/trades-integration.test.tsx`

**Dependencies:** `Task 1: Make resync completion surface-aware`

**Write Scope:** `fluxboard/Trades.tsx`, `fluxboard/lib/realtime/types.ts`, `fluxboard/__tests__/trades-status.test.tsx`, `fluxboard/Trades.test.tsx`, `fluxboard/__tests__/trades-integration.test.tsx`

**Verification Commands:**
- `VITEST_FULL=1 pnpm --dir fluxboard exec vitest run __tests__/trades-status.test.tsx Trades.test.tsx __tests__/trades-integration.test.tsx`

**Step 1: Write the failing tests**
- Add a test proving that a fresh non-canonical trades view is `LIVE`, not `RECOVERING`, when it intentionally lacks standard realtime lineage.
- Add a reconnect test proving the banner only appears while bounded catch-up is actually happening.
- Preserve one canonical-view regression proving real seq-gap recovery still shows `RECOVERING`.

**Step 2: Run tests to verify they fail**
- Run: `VITEST_FULL=1 pnpm --dir fluxboard exec vitest run __tests__/trades-status.test.tsx Trades.test.tsx __tests__/trades-integration.test.tsx`
- Expected: FAIL because the current `syncSurfaceState` logic treats missing `streamId` or `snapshotRevision` as recovery even for non-canonical views.

**Step 3: Write minimal implementation**
- In [Trades.tsx](/home/ubuntu/nautilus_trader/fluxboard/Trades.tsx), separate canonical realtime lineage health from snapshot freshness.
- Keep canonical live view fail-closed.
- Treat fresh non-canonical views as `LIVE`.
- Add a small debounce or hysteresis so reconnect or short replay transitions do not flash unnecessarily.

**Step 4: Run tests to verify they pass**
- Run: `VITEST_FULL=1 pnpm --dir fluxboard exec vitest run __tests__/trades-status.test.tsx Trades.test.tsx __tests__/trades-integration.test.tsx`
- Expected: PASS, with truthful `LIVE` behavior for non-canonical views and preserved recovery behavior for real gaps.

**Step 5: Commit**
- `git add fluxboard/Trades.tsx fluxboard/lib/realtime/types.ts fluxboard/__tests__/trades-status.test.tsx fluxboard/Trades.test.tsx fluxboard/__tests__/trades-integration.test.tsx`
- `git commit -m "fix(trades): make health state reflect actual recovery work"`

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 3: Complete TokenMM normalized quantity writes and compatibility gating

**Files:**
- Modify: `systems/flux/flux/strategies/shared/trades.py`
- Modify: `systems/flux/flux/api/_payloads_common.py`
- Modify: `systems/flux/flux/api/app.py`
- Modify: `systems/flux/flux/api/socketio.py`
- Test: `tests/unit_tests/flux/strategies/shared/test_trades.py`
- Test: `tests/unit_tests/flux/api/test_tokenmm_compat.py`
- Test: `tests/unit_tests/flux/api/test_socketio_tokenmm.py`
- Test: `tests/unit_tests/flux/api/test_realtime_contract.py`

**Dependencies:** `Task 2: Make trades health truthful for canonical and non-canonical views`

**Write Scope:** `systems/flux/flux/strategies/shared/trades.py`, `systems/flux/flux/api/_payloads_common.py`, `systems/flux/flux/api/app.py`, `systems/flux/flux/api/socketio.py`, `tests/unit_tests/flux/strategies/shared/test_trades.py`, `tests/unit_tests/flux/api/test_tokenmm_compat.py`, `tests/unit_tests/flux/api/test_socketio_tokenmm.py`, `tests/unit_tests/flux/api/test_realtime_contract.py`

**Verification Commands:**
- `pytest -q tests/unit_tests/flux/strategies/shared/test_trades.py tests/unit_tests/flux/api/test_tokenmm_compat.py`
- `pytest -q tests/unit_tests/flux/api/test_socketio_tokenmm.py tests/unit_tests/flux/api/test_realtime_contract.py -k "trade_gap or compatibility or trades"`

**Step 1: Write the failing tests**
- Add coverage proving all new TokenMM trade writes include `qty`, `qty_base`, `qty_venue`, `qty_conversion_status`, and `qty_conversion_source`.
- Add API and socket coverage proving a clean normalized stream does not advertise `compatibility_mode`.
- Preserve regressions that still expect `compatibility_mode=True` when a deliberately seeded legacy row is present.

**Step 2: Run tests to verify they fail**
- Run: `pytest -q tests/unit_tests/flux/strategies/shared/test_trades.py tests/unit_tests/flux/api/test_tokenmm_compat.py`
- Expected: FAIL where new-write or clean-stream expectations are not yet guaranteed end to end.

**Step 3: Write minimal implementation**
- In [trades.py](/home/ubuntu/nautilus_trader/systems/flux/flux/strategies/shared/trades.py), guarantee the normalized quantity contract on all new TokenMM trade payloads.
- In [_payloads_common.py](/home/ubuntu/nautilus_trader/systems/flux/flux/api/_payloads_common.py), keep compatibility detection strict but truthful.
- In [app.py](/home/ubuntu/nautilus_trader/systems/flux/flux/api/app.py) and [socketio.py](/home/ubuntu/nautilus_trader/systems/flux/flux/api/socketio.py), ensure clean normalized streams no longer force compatibility mode or `trade_gap`.

**Step 4: Run tests to verify they pass**
- Run: `pytest -q tests/unit_tests/flux/strategies/shared/test_trades.py tests/unit_tests/flux/api/test_tokenmm_compat.py`
- Run: `pytest -q tests/unit_tests/flux/api/test_socketio_tokenmm.py tests/unit_tests/flux/api/test_realtime_contract.py -k "trade_gap or compatibility or trades"`
- Expected: PASS, with compatibility mode reserved only for deliberately seeded legacy rows.

**Step 5: Commit**
- `git add systems/flux/flux/strategies/shared/trades.py systems/flux/flux/api/_payloads_common.py systems/flux/flux/api/app.py systems/flux/flux/api/socketio.py tests/unit_tests/flux/strategies/shared/test_trades.py tests/unit_tests/flux/api/test_tokenmm_compat.py tests/unit_tests/flux/api/test_socketio_tokenmm.py tests/unit_tests/flux/api/test_realtime_contract.py`
- `git commit -m "fix(tokenmm): complete normalized trades quantity contract"`

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 4: Dedupe recovery churn and document TokenMM stream cutover

**Files:**
- Modify: `fluxboard/Trades.tsx`
- Modify: `fluxboard/sockets.ts`
- Modify: `systems/flux/flux/api/socketio.py`
- Test: `fluxboard/__tests__/trades-integration.test.tsx`
- Test: `fluxboard/__tests__/realtime/compatibility-matrix.test.tsx`
- Test: `tests/unit_tests/flux/api/test_socketio_tokenmm.py`
- Test: `tests/unit_tests/flux/api/test_realtime_contract.py`
- Create: `docs/runbooks/tokenmm-trades-blotter-cutover.md`

**Dependencies:** `Task 3: Complete TokenMM normalized quantity writes and compatibility gating`

**Write Scope:** `fluxboard/Trades.tsx`, `fluxboard/sockets.ts`, `systems/flux/flux/api/socketio.py`, `fluxboard/__tests__/trades-integration.test.tsx`, `fluxboard/__tests__/realtime/compatibility-matrix.test.tsx`, `tests/unit_tests/flux/api/test_socketio_tokenmm.py`, `tests/unit_tests/flux/api/test_realtime_contract.py`, `docs/runbooks/tokenmm-trades-blotter-cutover.md`

**Verification Commands:**
- `VITEST_FULL=1 pnpm --dir fluxboard exec vitest run __tests__/trades-integration.test.tsx __tests__/realtime/compatibility-matrix.test.tsx`
- `pytest -q tests/unit_tests/flux/api/test_socketio_tokenmm.py tests/unit_tests/flux/api/test_realtime_contract.py -k "trade_gap or recovery_required or trades"`

**Step 1: Write the failing tests**
- Add a reconnect regression proving one reconnect opens one bounded recovery sequence rather than repeated recovery churn.
- Add focused backend tests proving repeated known `trade_gap` or reconnect conditions do not emit redundant recovery cycles for the same unchanged condition.
- Add a runbook stub for the one-time TokenMM stream cutover.

**Step 2: Run tests to verify they fail**
- Run: `VITEST_FULL=1 pnpm --dir fluxboard exec vitest run __tests__/trades-integration.test.tsx __tests__/realtime/compatibility-matrix.test.tsx`
- Run: `pytest -q tests/unit_tests/flux/api/test_socketio_tokenmm.py tests/unit_tests/flux/api/test_realtime_contract.py -k "trade_gap or recovery_required or trades"`
- Expected: FAIL because reconnect and recovery behavior still churns more than the final contract allows.

**Step 3: Write minimal implementation**
- In [sockets.ts](/home/ubuntu/nautilus_trader/fluxboard/sockets.ts), keep reconnect resync bumping but dedupe repeat bumps for the same active condition.
- In [Trades.tsx](/home/ubuntu/nautilus_trader/fluxboard/Trades.tsx), expose explicit recovery reasons and suppress redundant banner churn.
- In [socketio.py](/home/ubuntu/nautilus_trader/systems/flux/flux/api/socketio.py), preserve strict recovery on real gaps while avoiding repeated emission for the same unchanged legacy or cursor condition.
- In [tokenmm-trades-blotter-cutover.md](/home/ubuntu/nautilus_trader/docs/runbooks/tokenmm-trades-blotter-cutover.md), document the operational stream reset or equivalent cutover required to remove retained legacy rows after Task 3 is green.

**Step 4: Run tests to verify they pass**
- Run: `VITEST_FULL=1 pnpm --dir fluxboard exec vitest run __tests__/trades-integration.test.tsx __tests__/realtime/compatibility-matrix.test.tsx`
- Run: `pytest -q tests/unit_tests/flux/api/test_socketio_tokenmm.py tests/unit_tests/flux/api/test_realtime_contract.py -k "trade_gap or recovery_required or trades"`
- Expected: PASS, with one reconnect producing one bounded recovery path and the cutover runbook committed.

**Step 5: Commit**
- `git add fluxboard/Trades.tsx fluxboard/sockets.ts systems/flux/flux/api/socketio.py fluxboard/__tests__/trades-integration.test.tsx fluxboard/__tests__/realtime/compatibility-matrix.test.tsx tests/unit_tests/flux/api/test_socketio_tokenmm.py tests/unit_tests/flux/api/test_realtime_contract.py docs/runbooks/tokenmm-trades-blotter-cutover.md`
- `git commit -m "fix(trades): dedupe recovery churn and add cutover runbook"`

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 5: Verify focused suites and finalize trades rollout docs

**Files:**
- Modify: `docs/plans/realtime-surfaces/trades-cutover.md`
- Modify: `fluxboard/docs/tokenmm_socket_contract.md`
- Modify: `docs/plans/2026-03-26-trades-blotter-hardening.md`

**Dependencies:** `Task 1: Make resync completion surface-aware`, `Task 2: Make trades health truthful for canonical and non-canonical views`, `Task 3: Complete TokenMM normalized quantity writes and compatibility gating`, `Task 4: Dedupe recovery churn and document TokenMM stream cutover`

**Write Scope:** `docs/plans/realtime-surfaces/trades-cutover.md`, `fluxboard/docs/tokenmm_socket_contract.md`, `docs/plans/2026-03-26-trades-blotter-hardening.md`

**Verification Commands:**
- `VITEST_FULL=1 pnpm --dir fluxboard exec vitest run __tests__/resync-contract.test.tsx __tests__/TradesStore.test.ts __tests__/trades-status.test.tsx Trades.test.tsx __tests__/trades-integration.test.tsx __tests__/realtime/compatibility-matrix.test.tsx`
- `pytest -q tests/unit_tests/flux/strategies/shared/test_trades.py tests/unit_tests/flux/api/test_tokenmm_compat.py tests/unit_tests/flux/api/test_socketio_tokenmm.py tests/unit_tests/flux/api/test_realtime_contract.py`

**Step 1: Run the focused verification suite**
- Run the frontend and backend commands above.
- Record exact pass or fail results in the Progress Tracker.

**Step 2: Update rollout docs**
- Update [trades-cutover.md](/home/ubuntu/nautilus_trader/docs/plans/realtime-surfaces/trades-cutover.md) to describe the final steady-state contract.
- Update [tokenmm_socket_contract.md](/home/ubuntu/nautilus_trader/fluxboard/docs/tokenmm_socket_contract.md) so the resync ownership section no longer claims impossible `order-view` requirements on trades-only surfaces.

**Step 3: Update plan tracker with final notes**
- Record the verification commands, final diff references, and any rollout caveats in this plan document.

**Step 4: Commit**
- `git add docs/plans/realtime-surfaces/trades-cutover.md fluxboard/docs/tokenmm_socket_contract.md docs/plans/2026-03-26-trades-blotter-hardening.md`
- `git commit -m "docs(trades): finalize blotter hardening rollout contract"`

**Step 5: Rollout gate**
- Do not remove the last TokenMM compatibility UI copy in production until the runbook cutover has been executed and compatibility mode is observed false on the live stream.

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

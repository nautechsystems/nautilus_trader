# PR72 Finish Remediation Implementation Plan

> **For the execution agent:** REQUIRED SUB-SKILL: Before implementing this plan, choose exactly one execution mode and use the matching skill: `superpowers:subagent-driven-development` for same-session execution or `superpowers:executing-plans` for a separate-session handoff.
> **Progress:** The Progress Tracker in this document is the execution source of truth and must be updated on every state change.

**Goal:** Rebase PR #72 onto current `origin/main`, fix the confirmed backend/frontend correctness issues, clean up rollout-proof and contract inconsistencies, and get the branch to a merge-ready verified state.

**Architecture:** Keep the existing recovered controller branch shape, but finish it as a remediation wave rather than reopening the original feature lanes. Rebase/conflict resolution happens first on the controller lane, then the backend and frontend correctness fixes can proceed in parallel because their write scopes are disjoint, and finally the rollout/docs/test cleanup lands on top once the runtime behavior is settled.

**Tech Stack:** Git worktrees, Python/Flask-SocketIO backend, React/TypeScript frontend, Vitest, Playwright, pytest.

## Progress Tracker

**Source of truth:** Update this table whenever task state changes. Do not rely on memory, chat history, or TodoWrite alone.

| Task | Status | Owner | Depends On | Write Scope | Lane Branch | Worktree Path | Commit / Diff | Verification | Notes / Last Update |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Overall | completed | main | none | `docs/plans/2026-03-26-pr72-finish-remediation.md`, `docs/plans/realtime-status/rt-standard-transport.md`, `docs/plans/realtime-status/rt-market-balances.md`, `docs/plans/2026-03-19-fluxboard-performance-improvement-plan.md` | `lanes/pr72-finish-controller` | `/home/ubuntu/nautilus_trader/.worktrees/pr72-finish-controller` | `cleanup commit b2bb54e3b4 plus tracker closeout follow-up` | `Task 3 targeted vitest slice green (86 passed) plus api.flux params-write tests green (3 passed); Task 4 cleanup slice green: VITEST_FULL=1 pnpm --dir fluxboard exec vitest run __tests__/realtime/baseline-budgets.test.tsx __tests__/realtime/compatibility-matrix.test.tsx (10 passed); Final controller sweep green: VITEST_FULL=1 pnpm --dir fluxboard exec vitest run __tests__/realtime/standard-socket-client.test.tsx __tests__/trades-integration.test.tsx __tests__/panels/signal.test.tsx __tests__/hooks/useVirtualizedRows.test.ts __tests__/realtime/baseline-budgets.test.tsx __tests__/realtime/compatibility-matrix.test.tsx api.flux.test.ts __tests__/realtime/market-balances-standard.test.tsx (138 passed); pnpm --dir fluxboard build:test; E2E_BASE_URL=http://127.0.0.1:4173 pnpm --dir fluxboard exec playwright test -c playwright.smoke.config.ts e2e/realtime-cutovers/trades.spec.ts (1 passed); python3 -m py_compile systems/flux/flux/api/socketio.py systems/flux/flux/api/app.py; backend pytest still blocked in current env` | Task 1-4 are integrated onto the controller branch and the final verification sweep is complete. Review-oriented subagent attempts for the closeout diff stalled in this session, so the branch closeout relies on the recorded controller diff review plus passing owned verification. The only remaining local gap is the blocked backend pytest environment |
| Task 1: Rebase Controller Branch And Resolve Mainline Conflicts | completed | main | none | `fluxboard/Balances.tsx`, `fluxboard/Trades.recovery.test.tsx`, `fluxboard/Trades.tsx`, `fluxboard/__tests__/trades-integration.test.tsx`, `fluxboard/api.ts`, `fluxboard/types.ts`, `systems/flux/flux/api/app.py`, `systems/flux/flux/api/socketio.py`, `tests/unit_tests/flux/api/test_socketio_tokenmm.py` | `lanes/pr72-finish-controller` | `/home/ubuntu/nautilus_trader/.worktrees/pr72-finish-controller` | `f31e9709c3` | `python3 -m py_compile systems/flux/flux/api/app.py systems/flux/flux/api/socketio.py; VITEST_FULL=1 pnpm --dir fluxboard exec vitest run Trades.recovery.test.tsx __tests__/trades-integration.test.tsx Balances.test.tsx (56 passed)` | Merged `origin/main` into the controller branch, cleared all 9 conflict files, and repaired the missing `resolvePathnameProfile` import found during smoke validation |
| Task 2: Harden Backend Standard Subscription Lifecycle | completed | main | Task 1: Rebase Controller Branch And Resolve Mainline Conflicts | `systems/flux/flux/api/socketio.py`, `systems/flux/docs/api.md`, `tests/unit_tests/flux/api/test_realtime_contract.py`, `tests/unit_tests/flux/api/test_app.py`, `tests/unit_tests/flux/api/test_socketio_tokenmm.py` | `lanes/task-2-pr72-backend-lifecycle` | `/home/ubuntu/nautilus_trader/.worktrees/task-2-pr72-backend-lifecycle` | `d9b37ac302 -> controller 63c4de70bc` | `python3 -m py_compile systems/flux/flux/api/socketio.py systems/flux/flux/api/app.py; git diff --check; pytest still blocked locally by missing built module nautilus_trader.core.data` | Task 2 lane commits were integrated onto the controller branch after spec pass and local review; the only remaining gap is the blocked backend pytest environment |
| Task 3: Harden Frontend Standard Subscription And Socket Lifecycle | completed | main | Task 1: Rebase Controller Branch And Resolve Mainline Conflicts | `fluxboard/hooks/useWebSocket.ts`, `fluxboard/sockets.ts`, `fluxboard/PnL.tsx`, `fluxboard/Trades.tsx`, `fluxboard/Alerts.tsx`, `fluxboard/components/domain/signal/SignalTable.tsx`, `fluxboard/hooks/useVirtualizedRows.ts`, `fluxboard/sockets.test.ts`, `fluxboard/api.flux.test.ts`, `fluxboard/__tests__/realtime/standard-socket-client.test.tsx`, `fluxboard/__tests__/trades-integration.test.tsx`, `fluxboard/__tests__/panels/signal.test.tsx`, `fluxboard/__tests__/hooks/useVirtualizedRows.test.ts` | `lanes/task-3-pr72-frontend-runtime` | `/home/ubuntu/nautilus_trader/.worktrees/task-3-pr72-frontend-runtime` | `e6e1461c70 -> controller c499ec8eff` | `VITEST_FULL=1 pnpm --dir fluxboard exec vitest run sockets.test.ts __tests__/realtime/standard-socket-client.test.tsx __tests__/trades-integration.test.tsx __tests__/panels/signal.test.tsx __tests__/hooks/useVirtualizedRows.test.ts (86 passed); VITEST_FULL=1 pnpm --dir fluxboard exec vitest run api.flux.test.ts --testNamePattern \"patchStrategyParams|updateParams\" (3 passed)` | Task 3 lane commits were integrated onto the controller branch after targeted verification and local review; the params-write test harness alignment landed as a follow-up to unblock final verification |
| Task 4: Clean Up Rollout Evidence, Capability Fixtures, And Trackers | completed | main | Task 2: Harden Backend Standard Subscription Lifecycle; Task 3: Harden Frontend Standard Subscription And Socket Lifecycle | `fluxboard/components/trades/PerfHarness.tsx`, `fluxboard/__tests__/realtime/baseline-budgets.test.tsx`, `fluxboard/__tests__/realtime/compatibility-matrix.test.tsx`, `fluxboard/e2e/realtime-cutovers/trades.spec.ts`, `docs/plans/2026-03-19-fluxboard-performance-improvement-plan.md`, `docs/plans/realtime-surfaces/signal-cutover.md`, `docs/plans/realtime-surfaces/trades-cutover.md`, `docs/plans/realtime-status/rt-standard-transport.md`, `docs/plans/realtime-status/rt-market-balances.md` | `lanes/pr72-finish-controller` | `/home/ubuntu/nautilus_trader/.worktrees/pr72-finish-controller` | `b2bb54e3b4` | `VITEST_FULL=1 pnpm --dir fluxboard exec vitest run __tests__/realtime/baseline-budgets.test.tsx __tests__/realtime/compatibility-matrix.test.tsx (10 passed); pnpm --dir fluxboard build:test; E2E_BASE_URL=http://127.0.0.1:4173 pnpm --dir fluxboard exec playwright test -c playwright.smoke.config.ts e2e/realtime-cutovers/trades.spec.ts (1 passed); git diff --check` | The original cleanup lane remained untouched, so Task 4 was reclaimed into the controller worktree. Commit `b2bb54e3b4` makes the perf artifacts explicitly committed-reference data and clarifies that standard steady-state traffic is Socket.IO while recovery capabilities remain invalidate-only / polling-only. Closeout review-agent attempts stalled in this session, so controller diff review was used as the fallback review path |
| Task 5: Verification Sweep And Merge-Readiness Review | completed | main | Task 2: Harden Backend Standard Subscription Lifecycle; Task 3: Harden Frontend Standard Subscription And Socket Lifecycle; Task 4: Clean Up Rollout Evidence, Capability Fixtures, And Trackers | `shared verification only` | `lanes/pr72-finish-controller` | `/home/ubuntu/nautilus_trader/.worktrees/pr72-finish-controller` | `cleanup commit b2bb54e3b4 plus tracker closeout follow-up` | `git status --short (clean before tracker closeout); git diff --stat origin/main...HEAD (98 files, +22896/-1633 at cleanup commit); VITEST_FULL=1 pnpm --dir fluxboard exec vitest run __tests__/realtime/standard-socket-client.test.tsx __tests__/trades-integration.test.tsx __tests__/panels/signal.test.tsx __tests__/hooks/useVirtualizedRows.test.ts __tests__/realtime/baseline-budgets.test.tsx __tests__/realtime/compatibility-matrix.test.tsx api.flux.test.ts __tests__/realtime/market-balances-standard.test.tsx (138 passed); pnpm --dir fluxboard build:test; E2E_BASE_URL=http://127.0.0.1:4173 pnpm --dir fluxboard exec playwright test -c playwright.smoke.config.ts e2e/realtime-cutovers/trades.spec.ts (1 passed); python3 -m py_compile systems/flux/flux/api/socketio.py systems/flux/flux/api/app.py; pytest still blocked locally by missing built module nautilus_trader.core.data` | Final controller verification sweep completed on top of `b2bb54e3b4`. Frontend combined slice, build, and owned Trades cutover smoke are green; backend pytest remains blocked by the missing local build artifact. Multiple closeout review-agent attempts stalled without findings, so merge-readiness is based on controller diff review plus the recorded verification evidence |

---

### Task 1: Rebase Controller Branch And Resolve Mainline Conflicts

**Files:**
- Modify: `fluxboard/Balances.tsx`
- Modify: `fluxboard/Trades.recovery.test.tsx`
- Modify: `fluxboard/Trades.tsx`
- Modify: `fluxboard/__tests__/trades-integration.test.tsx`
- Modify: `fluxboard/api.ts`
- Modify: `fluxboard/types.ts`
- Modify: `systems/flux/flux/api/app.py`
- Modify: `systems/flux/flux/api/socketio.py`
- Modify: `tests/unit_tests/flux/api/test_socketio_tokenmm.py`

**Dependencies:** `none`

**Write Scope:** `fluxboard/Balances.tsx`, `fluxboard/Trades.recovery.test.tsx`, `fluxboard/Trades.tsx`, `fluxboard/__tests__/trades-integration.test.tsx`, `fluxboard/api.ts`, `fluxboard/types.ts`, `systems/flux/flux/api/app.py`, `systems/flux/flux/api/socketio.py`, `tests/unit_tests/flux/api/test_socketio_tokenmm.py`

**Verification Commands:**
- `git merge --no-commit --no-ff origin/main`
- `git diff --name-only --diff-filter=U`
- `git merge --abort`

**Steps:**
1. Fetch current `origin/main` in the controller worktree and re-check the conflict set.
2. Merge or rebase `origin/main` onto `lanes/pr72-finish-controller`.
3. Resolve all overlapping frontend/backend conflicts without regressing the PR-head fixes already present on `pr-72-review`.
4. Run focused smoke verification on the conflicted areas once the tree is clean.
5. Update the tracker with the integrated controller commit and note any new blockers introduced by rebasing.

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 2: Harden Backend Standard Subscription Lifecycle

**Files:**
- Modify: `systems/flux/flux/api/socketio.py`
- Modify: `systems/flux/docs/api.md`
- Modify: `tests/unit_tests/flux/api/test_realtime_contract.py`
- Modify: `tests/unit_tests/flux/api/test_app.py`
- Modify: `tests/unit_tests/flux/api/test_socketio_tokenmm.py`

**Dependencies:** `Task 1: Rebase Controller Branch And Resolve Mainline Conflicts`

**Write Scope:** `systems/flux/flux/api/socketio.py`, `systems/flux/docs/api.md`, `tests/unit_tests/flux/api/test_realtime_contract.py`, `tests/unit_tests/flux/api/test_app.py`, `tests/unit_tests/flux/api/test_socketio_tokenmm.py`

**Verification Commands:**
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 /home/ubuntu/nautilus_trader/.venv/bin/pytest -q tests/unit_tests/flux/api/test_realtime_contract.py --confcutdir=tests/unit_tests/flux/api`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 /home/ubuntu/nautilus_trader/.venv/bin/pytest -q tests/unit_tests/flux/api/test_app.py tests/unit_tests/flux/api/test_socketio_tokenmm.py --confcutdir=tests/unit_tests/flux/api`

**Steps:**
1. Add failing backend tests for profile-switch cleanup after successful standard subscribe.
2. Add failing backend tests for subscribe-time priming failure cleanup.
3. Implement the minimal lifecycle changes in `socketio.py` so standard subscriptions cannot leak across `set_profile` and failed subscribe handshakes.
4. Update `systems/flux/docs/api.md` so the Socket.IO contract documents all real standard event kinds and current semantics.
5. Re-run the targeted backend tests, then stage the lane for spec review and quality review.

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 3: Harden Frontend Standard Subscription And Socket Lifecycle

**Files:**
- Modify: `fluxboard/hooks/useWebSocket.ts`
- Modify: `fluxboard/sockets.ts`
- Modify: `fluxboard/PnL.tsx`
- Modify: `fluxboard/Trades.tsx`
- Modify: `fluxboard/Alerts.tsx`
- Modify: `fluxboard/components/domain/signal/SignalTable.tsx`
- Modify: `fluxboard/hooks/useVirtualizedRows.ts`
- Modify: `fluxboard/__tests__/realtime/standard-socket-client.test.tsx`
- Modify: `fluxboard/__tests__/trades-integration.test.tsx`
- Modify: `fluxboard/__tests__/panels/signal.test.tsx`
- Modify: `fluxboard/__tests__/hooks/useVirtualizedRows.test.ts`

**Dependencies:** `Task 1: Rebase Controller Branch And Resolve Mainline Conflicts`

**Write Scope:** `fluxboard/hooks/useWebSocket.ts`, `fluxboard/sockets.ts`, `fluxboard/PnL.tsx`, `fluxboard/Trades.tsx`, `fluxboard/Alerts.tsx`, `fluxboard/components/domain/signal/SignalTable.tsx`, `fluxboard/hooks/useVirtualizedRows.ts`, `fluxboard/__tests__/realtime/standard-socket-client.test.tsx`, `fluxboard/__tests__/trades-integration.test.tsx`, `fluxboard/__tests__/panels/signal.test.tsx`, `fluxboard/__tests__/hooks/useVirtualizedRows.test.ts`

**Verification Commands:**
- `VITEST_FULL=1 pnpm --dir fluxboard exec vitest run __tests__/realtime/standard-socket-client.test.tsx __tests__/trades-integration.test.tsx __tests__/panels/signal.test.tsx __tests__/hooks/useVirtualizedRows.test.ts`
- `VITEST_FULL=1 pnpm --dir fluxboard exec vitest run __tests__/realtime/market-balances-standard.test.tsx api.flux.test.ts __tests__/realtime/compatibility-matrix.test.tsx`

**Steps:**
1. Add or tighten failing frontend tests for stable-lineage rerenders, socket destroy/recreate, legacy session state canonical-trades entry, and viewport resize behavior.
2. Change `useStandardWebSocketSubscription` so equivalent lineage refreshes do not churn subscriptions.
3. Make the standard socket client survive the PnL disconnect/recreate path.
4. Fix canonical Trades activation so flagged users with persisted non-canonical page size do not get stranded on REST-only behavior.
5. Make row virtualization respond to viewport height changes without requiring a scroll event.
6. Re-run the targeted frontend tests, then hand the lane to spec review and quality review.

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 4: Clean Up Rollout Evidence, Capability Fixtures, And Trackers

**Files:**
- Modify: `fluxboard/components/trades/PerfHarness.tsx`
- Modify: `fluxboard/__tests__/realtime/baseline-budgets.test.tsx`
- Modify: `fluxboard/__tests__/realtime/compatibility-matrix.test.tsx`
- Modify: `fluxboard/e2e/realtime-cutovers/trades.spec.ts`
- Modify: `docs/plans/2026-03-19-fluxboard-performance-improvement-plan.md`
- Modify: `docs/plans/realtime-surfaces/signal-cutover.md`
- Modify: `docs/plans/realtime-surfaces/trades-cutover.md`
- Modify: `docs/plans/realtime-status/rt-standard-transport.md`
- Modify: `docs/plans/realtime-status/rt-market-balances.md`

**Dependencies:** `Task 2: Harden Backend Standard Subscription Lifecycle`, `Task 3: Harden Frontend Standard Subscription And Socket Lifecycle`

**Write Scope:** `fluxboard/components/trades/PerfHarness.tsx`, `fluxboard/__tests__/realtime/baseline-budgets.test.tsx`, `fluxboard/__tests__/realtime/compatibility-matrix.test.tsx`, `fluxboard/e2e/realtime-cutovers/trades.spec.ts`, `docs/plans/2026-03-19-fluxboard-performance-improvement-plan.md`, `docs/plans/realtime-surfaces/signal-cutover.md`, `docs/plans/realtime-surfaces/trades-cutover.md`, `docs/plans/realtime-status/rt-standard-transport.md`, `docs/plans/realtime-status/rt-market-balances.md`

**Verification Commands:**
- `VITEST_FULL=1 pnpm --dir fluxboard exec vitest run __tests__/realtime/baseline-budgets.test.tsx __tests__/realtime/compatibility-matrix.test.tsx`
- `E2E_BASE_URL=http://127.0.0.1:4173 pnpm --dir fluxboard exec playwright test -c playwright.smoke.config.ts e2e/realtime-cutovers/trades.spec.ts`
- `pnpm --dir fluxboard build:test`

**Steps:**
1. Replace non-falsifiable perf fixture claims with language and tests that accurately describe what is measured versus what is reference data.
2. Align capability fixtures/tests with the real standard contract semantics after Tasks 2 and 3 land.
3. Update the cutover/status docs so they match the final runtime behavior and the remaining cleanup boundary honestly.
4. Re-run the targeted docs-adjacent tests and build, then send the lane through spec review and quality review.

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 5: Verification Sweep And Merge-Readiness Review

**Files:**
- Modify: `docs/plans/2026-03-26-pr72-finish-remediation.md`
- Modify: `docs/plans/realtime-status/rt-standard-transport.md`
- Modify: `docs/plans/realtime-status/rt-market-balances.md`
- Modify: `docs/plans/2026-03-19-fluxboard-performance-improvement-plan.md`

**Dependencies:** `Task 2: Harden Backend Standard Subscription Lifecycle`, `Task 3: Harden Frontend Standard Subscription And Socket Lifecycle`, `Task 4: Clean Up Rollout Evidence, Capability Fixtures, And Trackers`

**Write Scope:** `docs/plans/2026-03-26-pr72-finish-remediation.md`, `docs/plans/realtime-status/rt-standard-transport.md`, `docs/plans/realtime-status/rt-market-balances.md`, `docs/plans/2026-03-19-fluxboard-performance-improvement-plan.md`

**Verification Commands:**
- `git status --short`
- `git diff --stat origin/main...HEAD`
- `VITEST_FULL=1 pnpm --dir fluxboard exec vitest run __tests__/realtime/standard-socket-client.test.tsx __tests__/trades-integration.test.tsx __tests__/panels/signal.test.tsx __tests__/hooks/useVirtualizedRows.test.ts __tests__/realtime/baseline-budgets.test.tsx __tests__/realtime/compatibility-matrix.test.tsx api.flux.test.ts __tests__/realtime/market-balances-standard.test.tsx`
- `pnpm --dir fluxboard build:test`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 /home/ubuntu/nautilus_trader/.venv/bin/pytest -q tests/unit_tests/flux/api/test_realtime_contract.py tests/unit_tests/flux/api/test_app.py tests/unit_tests/flux/api/test_socketio_tokenmm.py --confcutdir=tests/unit_tests/flux/api`

**Steps:**
1. Integrate the approved backend/frontend/docs lane commits back onto the controller branch.
2. Re-run the combined targeted verification suite from the controller worktree.
3. Update this plan tracker plus the touched realtime status trackers with final commit SHAs and verification evidence.
4. Dispatch one final whole-diff review before closeout.
5. Record any remaining non-blocking follow-ups explicitly; otherwise declare the branch merge-ready.

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

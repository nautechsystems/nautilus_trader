# TokenMM Prod Remediation Implementation Plan

> **For the execution agent:** REQUIRED SUB-SKILL: Before implementing this plan, choose exactly one execution mode and use the matching skill: `superpowers:subagent-driven-development` for same-session execution or `superpowers:executing-plans` for a separate-session handoff.
> **Progress:** The Progress Tracker in this document is the execution source of truth and must be updated on every state change.

**Goal:** Restore truthful TokenMM operator surfaces and stable prod behavior by fixing the trades regression, runner-state semantics, persistent blocker alerting, and the current Bitget stale-state/runtime path in a single remediation PR.

**Architecture:** Keep the current fail-closed direction where it protects correctness, but stop hiding actionable operator information. For trades, add an explicit compatibility path for legacy rows while proving the current MakerV3 producer writes normalized fields on the normal path. For runtime health, separate runner liveness from trading enablement, emit low-frequency alerts for persistent blockers, and harden the Bitget private-account/startup-reconciliation path that currently leaves process liveness and strategy freshness out of sync.

**Tech Stack:** Flask/Redis API, Fluxboard React/TypeScript frontend, MakerV3 Python strategy runtime, Nautilus live execution engine, Bitget adapter, pytest, Vitest.

## Progress Tracker

**Source of truth:** Update this table whenever task state changes. Do not rely on memory, chat history, or TodoWrite alone.

| Task | Status | Owner | Depends On | Write Scope | Lane Branch | Worktree Path | Commit / Diff | Verification | Notes / Last Update |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Task 1: Restore TokenMM trades compatibility without hiding legacy rows | completed | main | none | `systems/flux/flux/api/app.py`, `systems/flux/flux/api/socketio.py`, `systems/flux/flux/api/_payloads_common.py`, `fluxboard/api.ts`, `fluxboard/Trades.tsx`, `fluxboard/components/trades/TradesTable.tsx`, `tests/unit_tests/flux/api/test_tokenmm_compat.py`, `fluxboard/__tests__/trades-delta-reset.test.tsx`, `fluxboard/Trades.test.tsx`, `systems/flux/flux/strategies/shared/trades.py`, `tests/unit_tests/flux/strategies/shared/test_trades.py` | `fix/tokenmm-prod-remediation-20260326` | `/home/ubuntu/nautilus_trader/.worktrees/tokenmm-prod-remediation-20260326` | `71065f7b09` | `pytest -q tests/unit_tests/flux/api/test_tokenmm_compat.py tests/unit_tests/flux/strategies/shared/test_trades.py PASS; VITEST_FULL=1 pnpm --dir /home/ubuntu/nautilus_trader/.worktrees/tokenmm-prod-remediation-20260326/fluxboard exec vitest run Trades.test.tsx __tests__/trades-delta-reset.test.tsx PASS` | Compatibility contract landed; legacy TokenMM rows stay visible and UI shows degraded semantics banner |
| Task 2: Make `run` mean runner freshness rather than `bot_on` | completed | main | Task 1 | `systems/flux/flux/api/app.py`, `fluxboard/utils/strategyStatus.ts`, `fluxboard/components/domain/signal/SignalTable.tsx`, `tests/unit_tests/flux/api/test_app.py`, `fluxboard/Trades.test.tsx` | `fix/tokenmm-prod-remediation-20260326` | `/home/ubuntu/nautilus_trader/.worktrees/tokenmm-prod-remediation-20260326` | `b7496f21f3` | `pytest -q tests/unit_tests/flux/api/test_app.py -k "params_include_persisted_and_effective_bot_on_fields or params_ignore_stale_state_summary_when_augmenting_bot_on_fields or params_ignore_stale_on_stop_state_summary_when_augmenting_bot_on_fields" PASS; VITEST_FULL=1 pnpm --dir /home/ubuntu/nautilus_trader/.worktrees/tokenmm-prod-remediation-20260326/fluxboard exec vitest run Trades.test.tsx PASS` | Fixed stale `on_stop` override in params augmentation; no frontend code change required |
| Task 3: Emit low-frequency warning alerts for persistent quote blockers | completed | main | Task 2 | `systems/flux/flux/strategies/makerv3/strategy.py`, `systems/flux/flux/strategies/shared/alerts.py`, `tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py`, `tests/unit_tests/flux/api/test_payloads.py` | `fix/tokenmm-prod-remediation-20260326` | `/home/ubuntu/nautilus_trader/.worktrees/tokenmm-prod-remediation-20260326` | pending_commit | `pytest -q tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py tests/unit_tests/flux/api/test_payloads.py PASS` | Added warning-level, cooldown-gated `spot_borrow_cap` alerts and test-harness IB stubs so the scoped suite runs in isolation |
| Task 4: Remediate Bitget stale-state/runtime divergence | completed | main | Task 2 | `nautilus_trader/adapters/bitget/execution.py`, `tests/integration_tests/adapters/bitget/test_execution.py` | `fix/tokenmm-prod-remediation-20260326` | `/home/ubuntu/nautilus_trader/.worktrees/tokenmm-prod-remediation-20260326` | pending_commit | `pytest -q tests/integration_tests/adapters/bitget/test_execution.py PASS` | Root cause was adapter-only: UTA private account snapshots could violate `AccountBalance` invariants, and cold-cache UTA private positions could not resolve `PLUMEUSDT-PERP.BITGET` when only the provider knew the instrument |
| Task 5: Verify, write rollout notes, and prepare the remediation PR | in_progress | main | Task 1, Task 2, Task 3, Task 4 | `docs/plans/2026-03-26-tokenmm-prod-remediation.md` | `fix/tokenmm-prod-remediation-20260326` | `/home/ubuntu/nautilus_trader/.worktrees/tokenmm-prod-remediation-20260326` | `5c8f7b91d4` | in_progress | Starting final verification and PR prep after adapter remediation |

---

### Task 1: Restore TokenMM trades compatibility without hiding legacy rows

**Files:**
- Modify: `systems/flux/flux/api/app.py`
- Modify: `systems/flux/flux/api/socketio.py`
- Modify: `systems/flux/flux/api/_payloads_common.py`
- Modify: `fluxboard/api.ts`
- Modify: `fluxboard/Trades.tsx`
- Modify: `fluxboard/components/trades/TradesTable.tsx`
- Test: `tests/unit_tests/flux/api/test_tokenmm_compat.py`
- Test: `fluxboard/__tests__/trades-delta-reset.test.tsx`
- Test: `fluxboard/Trades.test.tsx`
- Modify or confirm normal-path producer behavior in: `systems/flux/flux/strategies/shared/trades.py`
- Test: `tests/unit_tests/flux/strategies/shared/test_trades.py`

**Dependencies:** `none`

**Write Scope:** `systems/flux/flux/api/app.py`, `systems/flux/flux/api/socketio.py`, `systems/flux/flux/api/_payloads_common.py`, `fluxboard/api.ts`, `fluxboard/Trades.tsx`, `fluxboard/components/trades/TradesTable.tsx`, `tests/unit_tests/flux/api/test_tokenmm_compat.py`, `fluxboard/__tests__/trades-delta-reset.test.tsx`, `fluxboard/Trades.test.tsx`, `systems/flux/flux/strategies/shared/trades.py`, `tests/unit_tests/flux/strategies/shared/test_trades.py`

**Verification Commands:**
- `pytest -q tests/unit_tests/flux/api/test_tokenmm_compat.py tests/unit_tests/flux/strategies/shared/test_trades.py`
- `VITEST_FULL=1 pnpm --dir /home/ubuntu/nautilus_trader/fluxboard exec vitest run Trades.test.tsx __tests__/trades-delta-reset.test.tsx`

**Step 1: Write the failing tests**
- Add backend coverage proving TokenMM legacy trade rows remain visible in compatibility mode rather than forcing `rows=[]`.
- Add frontend coverage proving snapshot responses with compatibility metadata do not render the generic empty state.
- Add or extend producer-path coverage proving normal MakerV3 trade rows include `qty_base`, `qty_venue`, `qty_conversion_status`, and `qty_conversion_source` when instrument metadata is available.

**Step 2: Run the focused tests to verify the current behavior fails the new expectations**
- Run the backend and frontend commands above.
- Expected before implementation: legacy-row compatibility assertions fail, while existing reset-oriented tests document the current regression.

**Step 3: Implement the minimal compatibility path**
- Change the TokenMM trades snapshot and delta handlers so legacy rows are returned in compatibility mode instead of being blanked.
- Preserve explicit metadata that the rows are legacy/degraded so the UI can label them accurately.
- Update socket/delta behavior so the trades page stays usable after reconnect and does not oscillate between empty and populated states.
- Plumb the compatibility/reset metadata through Fluxboard and render a TokenMM-specific degraded message instead of “No trades in selected filter.”

**Step 4: Re-run focused tests**
- Run the backend and frontend commands above.
- Expected after implementation: compatibility path passes and producer-field assertions pass on the normal MakerV3 path.

**Step 5: Commit**
- `git add systems/flux/flux/api/app.py systems/flux/flux/api/socketio.py systems/flux/flux/api/_payloads_common.py fluxboard/api.ts fluxboard/Trades.tsx fluxboard/components/trades/TradesTable.tsx systems/flux/flux/strategies/shared/trades.py tests/unit_tests/flux/api/test_tokenmm_compat.py tests/unit_tests/flux/strategies/shared/test_trades.py fluxboard/__tests__/trades-delta-reset.test.tsx fluxboard/Trades.test.tsx`
- `git commit -m "fix(tokenmm): restore trades compatibility for legacy rows"`

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 2: Make `run` mean runner freshness rather than `bot_on`

**Files:**
- Modify: `systems/flux/flux/api/app.py`
- Modify: `fluxboard/utils/strategyStatus.ts`
- Modify if needed: `fluxboard/components/domain/signal/SignalTable.tsx`
- Test: `tests/unit_tests/flux/api/test_app.py`
- Test: `fluxboard/Trades.test.tsx`

**Dependencies:** `Task 1: Restore TokenMM trades compatibility without hiding legacy rows`

**Write Scope:** `systems/flux/flux/api/app.py`, `fluxboard/utils/strategyStatus.ts`, `fluxboard/components/domain/signal/SignalTable.tsx`, `tests/unit_tests/flux/api/test_app.py`, `fluxboard/Trades.test.tsx`

**Verification Commands:**
- `pytest -q tests/unit_tests/flux/api/test_app.py -k "params_include_persisted_and_effective_bot_on_fields or params_ignore_stale_state_summary_when_augmenting_bot_on_fields or params_ignore_stale_on_stop_state_summary_when_augmenting_bot_on_fields"`
- `VITEST_FULL=1 pnpm --dir /home/ubuntu/nautilus_trader/fluxboard exec vitest run Trades.test.tsx`

**Step 1: Write the failing tests**
- Add API tests for the contradictory state observed live: stale state summary plus `params.bot_on=false` plus process liveness should not mislabel `run`.
- Add frontend status tests proving `run` reflects `running` freshness only, while trading enablement remains separately controlled by `bot_on` / blocked state.

**Step 2: Run tests to confirm the contradiction**
- Run the commands above.
- Expected before implementation: the current payload composition and/or UI badge derivation fails the new truthfulness assertions.

**Step 3: Implement the contract cleanup**
- Keep `running` tied to service/state freshness.
- Keep `persisted_bot_on`, `effective_bot_on`, and `bot_on_reason` as trading-gate fields.
- Remove or prevent stale-state fallthrough that synthesizes contradictory “running” reasons when the process is not exporting fresh state.
- Adjust Fluxboard badge logic only as needed to respect the corrected API contract.

**Step 4: Re-run tests**
- Run the commands above.
- Expected after implementation: `run` and trading state are independently truthful.

**Step 5: Commit**
- `git add systems/flux/flux/api/app.py fluxboard/utils/strategyStatus.ts fluxboard/components/domain/signal/SignalTable.tsx tests/unit_tests/flux/api/test_app.py fluxboard/Trades.test.tsx`
- `git commit -m "fix(tokenmm): separate runner health from bot_on state"`

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 3: Emit low-frequency warning alerts for persistent quote blockers

**Files:**
- Modify: `systems/flux/flux/strategies/makerv3/strategy.py`
- Modify if needed: `systems/flux/flux/strategies/shared/alerts.py`
- Test: `tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py`
- Test if alert payload shaping changes: `tests/unit_tests/flux/api/test_payloads.py`

**Dependencies:** `Task 2: Make 'run' mean runner freshness rather than 'bot_on'`

**Write Scope:** `systems/flux/flux/strategies/makerv3/strategy.py`, `systems/flux/flux/strategies/shared/alerts.py`, `tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py`, `tests/unit_tests/flux/api/test_payloads.py`

**Verification Commands:**
- `pytest -q tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py tests/unit_tests/flux/api/test_payloads.py`

**Step 1: Write the failing tests**
- Add a focused test for a persistent `spot_borrow_cap` blocker that should emit warning alerts on a cooldown rather than only appearing in state payloads.
- Preserve the existing rate-limit expectations so the fix does not spam alerts on every publish.

**Step 2: Run tests to confirm current alert absence**
- Run the command above.
- Expected before implementation: blocker state is present, but the new alert-expectation test fails.

**Step 3: Implement the warning alert path**
- Reuse the existing actionable/rate-limited alert machinery where possible.
- Emit warning-level alerts for persistent quote blockers with a stable key and cooldown.
- Keep the behavior low-frequency and transition-aware.

**Step 4: Re-run tests**
- Run the command above.
- Expected after implementation: persistent blocker alerts appear without introducing alert spam.

**Step 5: Commit**
- `git add systems/flux/flux/strategies/makerv3/strategy.py systems/flux/flux/strategies/shared/alerts.py tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py tests/unit_tests/flux/api/test_payloads.py`
- `git commit -m "fix(tokenmm): alert on persistent quote blockers"`

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 4: Remediate Bitget stale-state/runtime divergence

**Files:**
- Modify: `nautilus_trader/adapters/bitget/execution.py`
- Modify: `nautilus_trader/live/execution_engine.py`
- Modify if readiness semantics need refinement: `systems/flux/flux/runners/tokenmm/readiness.py`
- Test: `tests/integration_tests/adapters/bitget/test_execution.py`
- Test: `tests/unit_tests/live/test_execution_engine.py`
- Test if readiness summary changes: `tests/unit_tests/flux/runners/test_tokenmm_readiness.py`

**Dependencies:** `Task 2: Make 'run' mean runner freshness rather than 'bot_on'`

**Write Scope:** `nautilus_trader/adapters/bitget/execution.py`, `nautilus_trader/live/execution_engine.py`, `systems/flux/flux/runners/tokenmm/readiness.py`, `tests/integration_tests/adapters/bitget/test_execution.py`, `tests/unit_tests/live/test_execution_engine.py`, `tests/unit_tests/flux/runners/test_tokenmm_readiness.py`

**Verification Commands:**
- `pytest -q tests/integration_tests/adapters/bitget/test_execution.py tests/unit_tests/live/test_execution_engine.py tests/unit_tests/flux/runners/test_tokenmm_readiness.py`

**Step 1: Write the failing tests**
- Add adapter coverage for the specific Bitget private-account payload shape or sequencing that currently leaves account state inconsistent after restart.
- Add execution-engine coverage for the restart-time reconciliation path observed live so it does not silently degrade into stale state while the process remains up.
- Add readiness coverage only if the stale-state summary contract changes.

**Step 2: Run tests to verify the reproduction is captured**
- Run the command above.
- Expected before implementation: the new Bitget/account or reconciliation assertions fail, documenting the current stale-state divergence.

**Step 3: Implement the minimal runtime fix**
- Correct the Bitget private-account handling so valid account snapshots do not collapse into an inconsistent or missing account-state update.
- Tighten startup reconciliation behavior in the live execution engine only where the current Bitget restart path is producing false or misleading stale-state outcomes.
- Keep the change scoped to the observed Bitget stale-state / reconciliation path, not a broad adapter redesign.

**Step 4: Re-run tests**
- Run the command above.
- Expected after implementation: the Bitget restart path no longer produces the observed stale-state/runtime divergence under the reproduced conditions.

**Step 5: Commit**
- `git add nautilus_trader/adapters/bitget/execution.py nautilus_trader/live/execution_engine.py systems/flux/flux/runners/tokenmm/readiness.py tests/integration_tests/adapters/bitget/test_execution.py tests/unit_tests/live/test_execution_engine.py tests/unit_tests/flux/runners/test_tokenmm_readiness.py`
- `git commit -m "fix(tokenmm): harden bitget stale-state recovery"`

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 5: Verify, write rollout notes, and prepare the remediation PR

**Files:**
- Modify: `docs/plans/2026-03-26-tokenmm-prod-remediation.md`

**Dependencies:** `Task 1: Restore TokenMM trades compatibility without hiding legacy rows`, `Task 2: Make 'run' mean runner freshness rather than 'bot_on'`, `Task 3: Emit low-frequency warning alerts for persistent quote blockers`, `Task 4: Remediate Bitget stale-state/runtime divergence`

**Write Scope:** `docs/plans/2026-03-26-tokenmm-prod-remediation.md`

**Verification Commands:**
- `pytest -q tests/unit_tests/flux/api/test_tokenmm_compat.py tests/unit_tests/flux/api/test_app.py tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py tests/unit_tests/live/test_execution_engine.py tests/unit_tests/flux/runners/test_tokenmm_readiness.py tests/integration_tests/adapters/bitget/test_execution.py`
- `pnpm --dir /home/ubuntu/nautilus_trader/fluxboard exec vitest run Trades.test.tsx __tests__/trades-delta-reset.test.tsx`

**Step 1: Run the full focused verification set**
- Execute the verification commands above.
- Record exact pass/fail output in the Progress Tracker.

**Step 2: Update the plan tracker**
- Mark completed tasks with final SHAs, verification commands, and notes.

**Step 3: Write PR notes**
- Summarize the trades compatibility path, runner-semantic cleanup, blocker alerting, Bitget remediation, and the follow-up architecture issue for quantity normalization ownership.
- Call out any remaining operational caveats explicitly.

**Step 4: Commit plan metadata updates if needed**
- `git add docs/plans/2026-03-26-tokenmm-prod-remediation.md`
- `git commit -m "docs: finalize tokenmm prod remediation plan"`

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

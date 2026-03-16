# TokenMM Signal Source-Of-Truth And Bitget Reconciliation Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.
> **Progress:** The Progress Tracker in this document is the execution source of truth and must be updated on every state change.

**Goal:** Make TokenMM/MakerV3 Signal a coherent operator cockpit by rendering from one strategy quote/pricing truth, while also fixing Bitget perp position/reconciliation errors and duplicate-position inflation in balances/inventory.

**Architecture:** First, make the backend publish a coherent MakerV3 quote/pricing snapshot whose fields come from one quote-cycle truth and expose explicit signed pricing adjustments. Second, make Fluxboard render Signal from that snapshot without mixing live tops, cached pricing, and fallback derivations opportunistically. Third, fix the Bitget perp position/reconciliation pipeline so bogus `EXTERNAL`/duplicate positions do not poison balances, inventory skew, or Signal.

**Tech Stack:** Python, pytest, systemd live services, Flux API payload builders, Fluxboard React/TypeScript, Vitest.

## Progress Tracker

**Source of truth:** Update this table whenever task state changes. Do not rely on memory, chat history, or TodoWrite alone.

| Task | Status | Owner | Notes / Last Update |
| --- | --- | --- | --- |
| Overall | in_progress | codex | Tasks 1-3 complete; Task 4 started on Bitget startup reconciliation |
| Task 1: Freeze Signal source-of-truth contract | completed | codex | Commit `0b5d820d3d`; red-state verified: pytest has 1 failing spread contract, vitest has 1 failing maker-truth spread render |
| Task 2: Publish coherent MakerV3 quote snapshot | completed | codex | Commit `bb691a6b03`; focused backend suite green: `35 passed` for quote_engine + signal contract tests |
| Task 3: Make Signal render from one pricing truth | completed | codex | Focused Vitest green: `7 passed`; spread now follows maker quote snapshot used by operator-facing Our/Ref rows |
| Task 4: Fix Bitget perp startup position parsing/reconciliation | in_progress | codex | Investigating stale EXTERNAL + strategy-owned Bitget perp positions and startup quantity handling |
| Task 5: Eliminate duplicate position inflation in balances/inventory | not_started | unassigned | Plan created |
| Task 6: Verify live behavior, docs, and PR hygiene | not_started | unassigned | Plan created |

---

### Task 1: Freeze Signal source-of-truth contract

**Files:**
- Modify: `systems/flux/flux/api/_payloads_signals.py`
- Modify: `fluxboard/components/domain/signal/SignalTable.tsx`
- Modify: `tests/unit_tests/flux/api/test_signals_inventory_contract.py`
- Modify: `fluxboard/tests/signal/SignalTable.audit.test.tsx`
- Create or modify: `fluxboard/tests/signal/SignalTable.sourceOfTruth.test.tsx`

**Step 1: Write failing backend contract tests**

Add tests that assert:
- `pricing_adjustments[].skew_bps_signed` is the canonical signed shift.
- MakerV3 quote snapshot fields used for operator display come from one explicit quote snapshot path, not mixed from live tops plus cached place levels.
- `spread_net_bps`/market-vs-ref calculations must use the same quote snapshot source as the visible “Our” row when the UI is in maker-truth mode.

**Step 2: Run tests to verify they fail**

Run:
```bash
pytest -q tests/unit_tests/flux/api/test_signals_inventory_contract.py
cd fluxboard && pnpm vitest run tests/signal/SignalTable.audit.test.tsx tests/signal/SignalTable.sourceOfTruth.test.tsx
```

Expected:
- At least one new assertion fails because Signal/API still mix sources.

**Step 3: Document the contract inline in code comments**

Add succinct comments in backend/UI code stating:
- `skew_bps_signed` is source-of-truth for directional pricing shift.
- operator-facing maker quote rows, spread, and effective edges must come from the same quote snapshot epoch.

**Step 4: Re-run tests after comments-only changes**

Run the same commands and confirm failures remain focused on missing implementation, not test mistakes.

**Step 5: Commit**

```bash
git add tests/unit_tests/flux/api/test_signals_inventory_contract.py fluxboard/tests/signal/SignalTable.audit.test.tsx fluxboard/tests/signal/SignalTable.sourceOfTruth.test.tsx systems/flux/flux/api/_payloads_signals.py fluxboard/components/domain/signal/SignalTable.tsx
git commit -m "test: define tokenmm signal source-of-truth contract"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 2: Publish coherent MakerV3 quote snapshot

**Files:**
- Modify: `systems/flux/flux/strategies/makerv3/quote_engine.py`
- Modify: `systems/flux/flux/strategies/makerv3/publisher.py`
- Modify: `systems/flux/flux/api/_payloads_signals.py`
- Modify: `tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py`
- Modify: `tests/unit_tests/flux/api/test_signals_inventory_contract.py`

**Step 1: Write failing tests for mixed-epoch pricing**

Add tests that fail if:
- publisher refreshes `skew` without refreshing quote-cycle pricing fields or explicitly marking them stale/unavailable.
- API merges stale quote snapshot fields with fresher maker/ref tops into one operator-facing snapshot.

**Step 2: Run tests to verify they fail**

Run:
```bash
pytest -q tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py tests/unit_tests/flux/api/test_signals_inventory_contract.py
```

Expected:
- New tests fail on current mixed-epoch behavior.

**Step 3: Implement coherent quote snapshot publication**

Implementation goals:
- Strategy publishes one quote snapshot payload per quote cycle containing:
  - `place_bid`, `place_ask`
  - `cancel_bid`, `cancel_ask`
  - `maker_top_bid`, `maker_top_ask`
  - `ref_bid`, `ref_ask`
  - `base_bid_edge_bps`, `base_ask_edge_bps`
  - `eff_bid_edge_bps`, `eff_ask_edge_bps`
  - `skew_bps_signed`
  - `place_edge_bps`
  - snapshot `ts_ms`
  - explicit freshness/staleness semantics
- Publisher must not splice fresh skew into stale quote-cycle pricing without also updating the enclosing snapshot contract.
- API should pass through this coherent snapshot rather than synthesizing hybrid state whenever possible.

**Step 4: Run focused tests**

Run:
```bash
pytest -q tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py tests/unit_tests/flux/api/test_signals_inventory_contract.py
```

Expected:
- Pass.

**Step 5: Commit**

```bash
git add systems/flux/flux/strategies/makerv3/quote_engine.py systems/flux/flux/strategies/makerv3/publisher.py systems/flux/flux/api/_payloads_signals.py tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py tests/unit_tests/flux/api/test_signals_inventory_contract.py
git commit -m "fix: publish coherent makerv3 quote snapshot for signal"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 3: Make Signal render from one pricing truth

**Files:**
- Modify: `fluxboard/components/domain/signal/SignalTable.tsx`
- Modify: `fluxboard/types.ts`
- Modify: `fluxboard/tests/signal/SignalTable.audit.test.tsx`
- Modify: `fluxboard/tests/signal/SignalTable.sourceOfTruth.test.tsx`

**Step 1: Write failing UI tests**

Cover:
- visible “Our” row, spread cell, tooltip edges, and skew all reflect the same quote snapshot epoch.
- stale snapshots are labeled as stale without mixing them with fresh maker/ref tops.
- when `skew_bps_signed` exists, UI never re-derives directional skew for display.

**Step 2: Run tests to verify they fail**

Run:
```bash
cd fluxboard && pnpm vitest run tests/signal/SignalTable.audit.test.tsx tests/signal/SignalTable.sourceOfTruth.test.tsx
```

Expected:
- Failures on current mixed rendering.

**Step 3: Implement UI cleanup**

Implementation goals:
- One helper resolves the canonical maker quote snapshot for rendering.
- spread cell uses the same maker quote snapshot as the visible “Our” row.
- do not prefer `maker_top_*` over `place_*` when the UI is meant to show quoting truth.
- preserve `skew_bps_signed` as canonical directional display.

**Step 4: Run focused tests**

Run:
```bash
cd fluxboard && pnpm vitest run tests/signal/SignalTable.audit.test.tsx tests/signal/SignalTable.sourceOfTruth.test.tsx
```

Expected:
- Pass.

**Step 5: Commit**

```bash
git add fluxboard/components/domain/signal/SignalTable.tsx fluxboard/types.ts fluxboard/tests/signal/SignalTable.audit.test.tsx fluxboard/tests/signal/SignalTable.sourceOfTruth.test.tsx
git commit -m "fix: align tokenmm signal rendering with strategy quote truth"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 4: Fix Bitget perp startup position parsing/reconciliation

**Files:**
- Modify: `nautilus_trader/adapters/bitget/execution.py`
- Modify: `nautilus_trader/live/execution_engine.py`
- Modify: `tests/unit_tests/live/test_execution_engine.py`
- Create or modify: `tests/unit_tests/adapters/bitget/test_execution_positions.py`
- Modify: `systems/flux/flux/strategies/makerv3/strategy.py`

**Step 1: Write failing adapter/execution tests**

Cover:
- Bitget UTA perp startup position payload parsing for `holdSide`, `total`, and quantity units.
- startup reconciliation does not leave a stale `EXTERNAL` position active alongside the strategy-owned position for the same effective venue position.
- if both appear, the engine cleans or dedupes them deterministically.

**Step 2: Run tests to verify they fail**

Run:
```bash
pytest -q tests/unit_tests/adapters/bitget/test_execution_positions.py tests/unit_tests/live/test_execution_engine.py
```

Expected:
- Failures reproducing current Bitget behavior.

**Step 3: Implement minimal root-cause fix**

Implementation goals:
- verify whether Bitget UTA perp `total` is venue qty, base qty, or another measure; parse it correctly.
- ensure reconciliation does not preserve stale `EXTERNAL` artifacts once the strategy-owned position is known.
- preserve valid reconciliation behavior for true missing positions.

**Step 4: Add strategy-side guardrail**

If duplicate/stale reports can still leak through, make MakerV3 local position summary detect and reject clearly conflicting same-instrument position states instead of blindly trusting the latest polluted snapshot.

**Step 5: Run focused tests**

Run:
```bash
pytest -q tests/unit_tests/adapters/bitget/test_execution_positions.py tests/unit_tests/live/test_execution_engine.py
```

Expected:
- Pass.

**Step 6: Commit**

```bash
git add nautilus_trader/adapters/bitget/execution.py nautilus_trader/live/execution_engine.py tests/unit_tests/live/test_execution_engine.py tests/unit_tests/adapters/bitget/test_execution_positions.py systems/flux/flux/strategies/makerv3/strategy.py
git commit -m "fix: correct bitget perp startup position reconciliation"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 5: Eliminate duplicate position inflation in balances/inventory

**Files:**
- Modify: `systems/flux/flux/common/portfolio_snapshot.py`
- Modify: `systems/flux/flux/api/_payloads_balances.py`
- Modify: `systems/flux/flux/strategies/makerv3/inventory.py`
- Modify: `tests/unit_tests/flux/api/test_payloads.py`
- Create or modify: `tests/unit_tests/flux/common/test_portfolio_snapshot.py`

**Step 1: Write failing aggregation tests**

Cover:
- same-instrument duplicate positions with `EXTERNAL` and strategy-owned rows do not double-count in portfolio/balances when they represent the same effective venue position.
- balances view remains transparent about reconciliation artifacts if they are intentionally retained.
- inventory skew consumes deduped/correct local/global quantities.

**Step 2: Run tests to verify they fail**

Run:
```bash
pytest -q tests/unit_tests/flux/common/test_portfolio_snapshot.py tests/unit_tests/flux/api/test_payloads.py
```

Expected:
- Fail on duplicate inflation or missing dedupe semantics.

**Step 3: Implement aggregation hardening**

Implementation goals:
- choose the correct dedupe layer after root cause is known:
  - execution engine cleanup if artifacts should never survive, and/or
  - portfolio snapshot / balances merge safeguards if duplicate rows still appear.
- do not hide genuinely distinct positions.
- preserve operator observability for abnormal reconciliation states.

**Step 4: Run focused tests**

Run:
```bash
pytest -q tests/unit_tests/flux/common/test_portfolio_snapshot.py tests/unit_tests/flux/api/test_payloads.py
```

Expected:
- Pass.

**Step 5: Commit**

```bash
git add systems/flux/flux/common/portfolio_snapshot.py systems/flux/flux/api/_payloads_balances.py systems/flux/flux/strategies/makerv3/inventory.py tests/unit_tests/flux/common/test_portfolio_snapshot.py tests/unit_tests/flux/api/test_payloads.py
git commit -m "fix: prevent duplicate position inflation in tokenmm balances"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 6: Verify live behavior, docs, and PR hygiene

**Files:**
- Modify: `systems/flux/docs/makerv3.md`
- Modify: `fluxboard/docs/tokenmm_contract.md`
- Modify: current PR description if needed

**Step 1: Run the full targeted verification set**

Run:
```bash
pytest -q \
  tests/unit_tests/flux/strategies/makerv3/test_pricing.py \
  tests/unit_tests/flux/strategies/makerv3/test_runtime_params.py \
  tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py \
  tests/unit_tests/flux/api/test_payloads.py \
  tests/unit_tests/flux/api/test_signals_inventory_contract.py \
  tests/unit_tests/flux/common/test_portfolio_snapshot.py \
  tests/unit_tests/adapters/bitget/test_execution_positions.py \
  tests/unit_tests/live/test_execution_engine.py

cd fluxboard && pnpm vitest run \
  tests/signal/SignalTable.audit.test.tsx \
  tests/signal/SignalTable.sourceOfTruth.test.tsx \
  __tests__/config/paramsProfiles.test.ts
```

Expected:
- All targeted tests pass.

**Step 2: Redeploy and smoke test live TokenMM**

Run:
```bash
pnpm --dir fluxboard build
sudo -n TOKENMM_DEPLOY_ROOT=/home/ubuntu/nautilus_trader TOKENMM_API_HOST=0.0.0.0 /home/ubuntu/nautilus_trader/ops/scripts/deploy/install_tokenmm_systemd.sh
sudo -n systemctl restart flux@tokenmm-api.service flux@tokenmm-bridge.service flux@tokenmm-portfolio.service
curl -I -fsS http://127.0.0.1:5022/tokenmm/signal
curl -fsS 'http://127.0.0.1:5022/api/v1/signals?profile=tokenmm'
curl -fsS 'http://127.0.0.1:5022/api/v1/balances?profile=tokenmm'
```

Expected:
- `200 OK`
- Signal rows consistent with backend snapshot
- Bitget perp qty no longer inflated if root cause fixed

**Step 3: Update docs**

Document:
- Signal source-of-truth contract
- meaning of `skew_bps_signed`
- operator interpretation of place prices vs maker top vs reference prices
- Bitget reconciliation expectations if relevant

**Step 4: Clean staging and PR**

Stage only files relevant to this workstream, excluding unrelated local changes unless they were intentionally part of the fix.

**Step 5: Final commit / PR update**

```bash
git add systems/flux/docs/makerv3.md fluxboard/docs/tokenmm_contract.md
git commit -m "docs: clarify tokenmm signal pricing truth and reconciliation"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

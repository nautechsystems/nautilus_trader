# TokenMM Spread Raw Mid Implementation Plan

> **For the execution agent:** REQUIRED SUB-SKILL: Before implementing this plan, choose exactly one execution mode and use the matching skill: `superpowers:subagent-driven-development` for same-session execution or `superpowers:executing-plans` for a separate-session handoff.
> **Progress:** The Progress Tracker in this document is the execution source of truth and must be updated on every state change.

**Goal:** Restore `spread_net_bps` to raw maker mid vs ref/FV mid for `maker_v3` while keeping `decision_edge_bps` quote-oriented.

**Architecture:** Split raw-spread and quoted-spread computation in the signal payload builder. Keep the `Spread` UI aligned with raw maker-top/reference mids and leave quote translation to decision-edge and skew fields.

**Tech Stack:** Python, pytest, TypeScript, Vitest, Flux API payload builders, Fluxboard signal table.

## Progress Tracker

**Source of truth:** Update this table whenever task state changes. Do not rely on memory, chat history, or TodoWrite alone.

| Task | Status | Owner | Depends On | Write Scope | Lane Branch | Worktree Path | Commit / Diff | Verification | Notes / Last Update |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Task 1: Update spread semantics regressions | completed | main | none | `tests/unit_tests/flux/api/test_signals_inventory_contract.py`, `fluxboard/tests/signal/SignalTable.audit.test.tsx`, `fluxboard/tests/signal/SignalTable.sourceOfTruth.test.tsx` | shared | shared | none | `pytest -q tests/unit_tests/flux/api/test_signals_inventory_contract.py -k 'raw_spread or spread_contract' PASS`; `pnpm vitest run tests/signal/SignalTable.audit.test.tsx tests/signal/SignalTable.sourceOfTruth.test.tsx PASS` | Regressions were flipped red first, then passed after payload/UI change |
| Task 2: Restore raw spread semantics in payload/UI | completed | main | Task 1: Update spread semantics regressions | `systems/flux/flux/api/_payloads_signals.py`, `fluxboard/components/domain/signal/SignalTable.tsx` | shared | shared | none | `pytest -q tests/unit_tests/flux/api/test_signals_inventory_contract.py -k 'raw_spread or spread_contract' PASS`; `pnpm vitest run tests/signal/SignalTable.audit.test.tsx tests/signal/SignalTable.sourceOfTruth.test.tsx PASS` | `spread_net_bps` now uses raw maker-top mid vs ref mid; UI spread cell matches |
| Task 3: Verify targeted and broader suites | completed | main | Task 2: Restore raw spread semantics in payload/UI | `systems/flux/flux/api/_payloads_signals.py`, `fluxboard/components/domain/signal/SignalTable.tsx`, related tests | shared | shared | none | `pytest -q tests/unit_tests/flux/api/test_signals_inventory_contract.py -k 'raw_spread or spread_contract' PASS`; `pytest -q tests/unit_tests/flux/api/test_payloads.py -k 'uses_injected_metadata_and_legs' PASS`; `pnpm vitest run tests/signal/SignalTable.audit.test.tsx tests/signal/SignalTable.sourceOfTruth.test.tsx components/SignalTable.test.tsx PASS`; `pnpm --dir fluxboard build PASS` | Broader `pytest` sweep hit unrelated pre-existing failures outside this write scope; spread-adjacent checks passed |

---

### Task 1: Update spread semantics regressions

**Files:**
- Modify: `tests/unit_tests/flux/api/test_signals_inventory_contract.py`
- Modify: `fluxboard/tests/signal/SignalTable.audit.test.tsx`
- Modify: `fluxboard/tests/signal/SignalTable.sourceOfTruth.test.tsx`

**Dependencies:** `none`

**Write Scope:** `tests/unit_tests/flux/api/test_signals_inventory_contract.py`, `fluxboard/tests/signal/SignalTable.audit.test.tsx`, `fluxboard/tests/signal/SignalTable.sourceOfTruth.test.tsx`

**Verification Commands:**
- `pytest -q tests/unit_tests/flux/api/test_signals_inventory_contract.py -k spread`
- `pnpm vitest run fluxboard/tests/signal/SignalTable.audit.test.tsx fluxboard/tests/signal/SignalTable.sourceOfTruth.test.tsx`

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 2: Restore raw spread semantics in payload/UI

**Files:**
- Modify: `systems/flux/flux/api/_payloads_signals.py`
- Modify: `fluxboard/components/domain/signal/SignalTable.tsx`

**Dependencies:** `Task 1: Update spread semantics regressions`

**Write Scope:** `systems/flux/flux/api/_payloads_signals.py`, `fluxboard/components/domain/signal/SignalTable.tsx`

**Verification Commands:**
- `pytest -q tests/unit_tests/flux/api/test_signals_inventory_contract.py -k spread`
- `pnpm vitest run fluxboard/tests/signal/SignalTable.audit.test.tsx fluxboard/tests/signal/SignalTable.sourceOfTruth.test.tsx`

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 3: Verify targeted and broader suites

**Files:**
- Modify: `docs/plans/2026-03-19-tokenmm-spread-raw-mid.md`

**Dependencies:** `Task 2: Restore raw spread semantics in payload/UI`

**Write Scope:** `systems/flux/flux/api/_payloads_signals.py`, `fluxboard/components/domain/signal/SignalTable.tsx`, `tests/unit_tests/flux/api/test_signals_inventory_contract.py`, `fluxboard/tests/signal/SignalTable.audit.test.tsx`, `fluxboard/tests/signal/SignalTable.sourceOfTruth.test.tsx`, `docs/plans/2026-03-19-tokenmm-spread-raw-mid.md`

**Verification Commands:**
- `pytest -q tests/unit_tests/flux/api/test_signals_inventory_contract.py tests/unit_tests/flux/api/test_payloads.py`
- `pnpm vitest run fluxboard/tests/signal/SignalTable.audit.test.tsx fluxboard/tests/signal/SignalTable.sourceOfTruth.test.tsx`

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

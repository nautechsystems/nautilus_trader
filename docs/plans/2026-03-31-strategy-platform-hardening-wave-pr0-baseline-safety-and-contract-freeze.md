# Strategy Platform Hardening Wave PR0 Baseline Safety And Contract Freeze Implementation Plan

> **For the execution agent:** REQUIRED SUB-SKILL: Before implementing this plan, choose exactly one execution mode and use the matching skill: `superpowers:subagent-driven-development` for same-session execution or `superpowers:executing-plans` for a separate-session handoff.
> **Progress:** The Progress Tracker in this document is the execution source of truth and must be updated on every state change.

**Goal:** Make the baseline releasable by fixing the current Makerv3 borrow-cap safety-path mismatch, freezing the venue-policy and quote-blocker contract, and proving the safety path is multi-market rather than PLUME-shaped.

**Architecture:** Add one explicit shared venue-policy helper for borrow-cap and exchange-code parsing, migrate Makerv3 onto it, and lock the alert/state/API contract in tests before any broader extraction work starts. This PR is allowed to change behavior only for the currently failing borrow-cap path, and only to align the implementation with the frozen contract below.

**Tech Stack:** Python strategy/runtime code, shared strategy helpers, Flux API payload builders, pytest.

**Context Docs:**
- Design: `docs/plans/2026-03-31-strategy-platform-hardening-wave-design.md`
- PRD: `docs/prd/2026-03-31-strategy-platform-hardening-wave.md`
- Relevant specs/runbooks: `systems/flux/docs/makerv3.md`, `docs/plans/2026-03-31-strategy-platform-hardening-wave.md`

**Decision Summary:**
- `PR0` is mandatory because the current Makerv3 suite is not releasable.
- Venue-policy parsing moves into an explicit shared helper now rather than after other extractions.
- The public operator and API contract for quote blockers must be pinned in the same PR that fixes the failing path.
- The shared venue-policy helper must return a structured result like `reason_code`, `affected_side`, and `venue_code`; raw rejection text may be preserved for logs and alerts but not re-parsed downstream.

## Borrow-Cap Contract Frozen In This PR

This PR must implement and freeze this exact contract:

- a spot borrow-cap rejection blocks only the affected ask or `SELL` side
- only affected ask managed orders are cancelled
- overall strategy state remains `running`
- `bot_on` remains `true`
- state and API payloads record a side-local `spot_borrow_cap` blocker
- one actionable alert plus structured event is emitted under cooldown gating

This PR does not get to defer that choice to implementation time.

## Release Gates

| Environment | Required for merge? | Requirement |
| --- | --- | --- |
| `default-unit` | yes | Makerv3, API, and readiness proof must be green |
| `pilot` | yes | deploy one pinned tokenmm pilot release from the PR head and validate the smoke bundle below |

## Affected Pilot Surfaces

- tokenmm Makerv3 strategy stack
- `/api/v1/signals` blocked-state consumers
- readiness and bridge consumers of `quote_blockers` truth

## Pilot Deploy Units And Promotion Order

Deploy units:

- `flux.runners.tokenmm.run_node`
- `flux.api.app` if pilot signals or readiness surfaces are served separately

Promotion order:

1. build one pinned pilot release from the exact PR head
2. deploy `flux.runners.tokenmm.run_node` and `flux.api.app` from that same release when both are in scope
3. validate blocker-state, readiness, and alert-cooldown behavior
4. promote the same units together only after the smoke bundle is clean

## Pilot Smoke Bundle

1. Deploy the tokenmm pilot stack from the exact PR head.
2. Confirm the strategy starts and remains `running`.
3. Confirm a borrow-cap rejection or controlled replay fixture produces an ask-side-only `spot_borrow_cap` blocker.
4. Confirm `/api/v1/signals` and readiness surfaces remain coherent with the same blocker state.
5. Confirm the alert path emits at most one actionable alert per cooldown window.

## PR-Local Rollback

- revert the whole PR
- redeploy the previous pinned pilot release
- do not mix reverted Makerv3 logic with new blocker-contract consumers

## Progress Tracker

**Source of truth:** Update this table whenever task state changes. Do not rely on memory, chat history, or TodoWrite alone.

| Task | Status | Owner | Depends On | Write Scope | Lane Branch | Worktree Path | Commit / Diff | Verification | Notes / Last Update |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Overall | not_started | unassigned | none | `systems/flux/flux/strategies/makerv3`, `systems/flux/flux/strategies/shared`, `systems/flux/flux/api`, `tests/unit_tests/flux/strategies/makerv3`, `tests/unit_tests/flux/strategies/shared`, `tests/unit_tests/flux/api`, `systems/flux/docs` | `wave/pr0-baseline-safety` | `.worktrees/strategy-platform-pr0` | none | not_run | Plan created |
| Task 1: Lock the failing borrow-cap contract in red tests | not_started | unassigned | none | `tests/unit_tests/flux/strategies/makerv3/test_order_safety.py`, `tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py`, `tests/unit_tests/flux/api/test_payloads.py`, `tests/unit_tests/flux/api/test_payload_snapshots.py`, `systems/flux/docs/makerv3.md` | `wave/pr0-baseline-safety` | `.worktrees/strategy-platform-pr0` | none | not_run | Plan created |
| Task 2: Extract shared venue-policy parsing and migrate Makerv3 | not_started | unassigned | Task 1: Lock the failing borrow-cap contract in red tests | `systems/flux/flux/strategies/shared/venue_policy.py`, `systems/flux/flux/strategies/makerv3/failures.py`, `systems/flux/flux/strategies/makerv3/strategy.py`, `tests/unit_tests/flux/strategies/shared/test_venue_policy.py` | `wave/pr0-baseline-safety` | `.worktrees/strategy-platform-pr0` | none | not_run | Plan created |
| Task 3: Align alerts, state exports, and API blocker semantics | not_started | unassigned | Task 2: Extract shared venue-policy parsing and migrate Makerv3 | `systems/flux/flux/strategies/makerv3/strategy.py`, `systems/flux/flux/strategies/makerv3/publisher.py`, `systems/flux/flux/api/_payloads_signals.py`, `tests/unit_tests/flux/api/test_payloads.py`, `systems/flux/docs/makerv3.md` | `wave/pr0-baseline-safety` | `.worktrees/strategy-platform-pr0` | none | not_run | Plan created |
| Task 4: Run baseline verification and write rollback note | not_started | unassigned | Task 3: Align alerts, state exports, and API blocker semantics | `docs/plans/2026-03-31-strategy-platform-hardening-wave-pr0-baseline-safety-and-contract-freeze.md`, `systems/flux/docs/makerv3.md` | `wave/pr0-baseline-safety` | `.worktrees/strategy-platform-pr0` | none | not_run | Plan created |

---

### Task 1: Lock the failing borrow-cap contract in red tests

**Files:**
- Modify: `tests/unit_tests/flux/strategies/makerv3/test_order_safety.py`
- Modify: `tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py`
- Modify: `tests/unit_tests/flux/api/test_payloads.py`
- Create: `tests/unit_tests/flux/api/test_payload_snapshots.py`
- Modify: `systems/flux/docs/makerv3.md`

**Dependencies:** `none`

**Write Scope:** `tests/unit_tests/flux/strategies/makerv3/test_order_safety.py`, `tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py`, `tests/unit_tests/flux/api/test_payloads.py`, `tests/unit_tests/flux/api/test_payload_snapshots.py`, `systems/flux/docs/makerv3.md`

**Verification Commands:**
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/strategies/makerv3/test_order_safety.py tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py -q`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/api/test_payloads.py -q -k 'quote_blockers or borrow'`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/api/test_payload_snapshots.py -q -k 'borrow_cap or quote_blockers'`

**Step 1: Write the failing tests**

Lock the intended contract for:

- which side is blocked on spot borrow cap
- whether an actionable alert is emitted
- which blocker fields appear in state and API payloads
- one representative golden payload fixture for blocked-state export
- multi-market behavior using non-PLUME fixture symbols

**Step 2: Run tests to verify they fail**

Run the commands above and confirm the current borrow-cap path is red.

**Step 3: Record the contract in docs**

Update `systems/flux/docs/makerv3.md` to state the frozen borrow-cap operator contract before implementation changes land.

**Step 4: Commit**

```bash
git add tests/unit_tests/flux/strategies/makerv3/test_order_safety.py \
  tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py \
  tests/unit_tests/flux/api/test_payloads.py \
  tests/unit_tests/flux/api/test_payload_snapshots.py \
  systems/flux/docs/makerv3.md
git commit -m "test: lock borrow cap safety contract"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 2: Extract shared venue-policy parsing and migrate Makerv3

**Files:**
- Create: `systems/flux/flux/strategies/shared/venue_policy.py`
- Modify: `systems/flux/flux/strategies/makerv3/failures.py`
- Modify: `systems/flux/flux/strategies/makerv3/strategy.py`
- Create: `tests/unit_tests/flux/strategies/shared/test_venue_policy.py`

**Dependencies:** `Task 1: Lock the failing borrow-cap contract in red tests`

**Write Scope:** `systems/flux/flux/strategies/shared/venue_policy.py`, `systems/flux/flux/strategies/makerv3/failures.py`, `systems/flux/flux/strategies/makerv3/strategy.py`, `tests/unit_tests/flux/strategies/shared/test_venue_policy.py`

**Verification Commands:**
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/strategies/shared/test_venue_policy.py -q`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/strategies/makerv3/test_order_safety.py -q -k 'borrow_cap or spot_borrow'`

**Step 1: Add the shared venue-policy tests**

Cover:

- exchange-code extraction
- borrow-cap reason detection
- non-PLUME symbols using the same reason parsing
- refusal to infer instrument family from symbol text

**Step 2: Implement the shared helper**

Move borrow-cap and exchange-code parsing into `shared/venue_policy.py`. Keep the helper pure and narrow, and return a structured result such as `reason_code`, `affected_side`, and `venue_code` rather than making later callers parse human text again.

**Step 3: Migrate Makerv3 onto the helper**

Replace direct parsing logic in `makerv3/failures.py` and related call sites with imports from the shared module.

**Step 4: Commit**

```bash
git add systems/flux/flux/strategies/shared/venue_policy.py \
  systems/flux/flux/strategies/makerv3/failures.py \
  systems/flux/flux/strategies/makerv3/strategy.py \
  tests/unit_tests/flux/strategies/shared/test_venue_policy.py
git commit -m "feat: add shared venue policy parsing"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 3: Align alerts, state exports, and API blocker semantics

**Files:**
- Modify: `systems/flux/flux/strategies/makerv3/strategy.py`
- Modify: `systems/flux/flux/strategies/makerv3/publisher.py`
- Modify: `systems/flux/flux/api/_payloads_signals.py`
- Modify: `tests/unit_tests/flux/api/test_payloads.py`
- Modify: `tests/unit_tests/flux/api/test_payload_snapshots.py`
- Modify: `systems/flux/docs/makerv3.md`

**Dependencies:** `Task 2: Extract shared venue-policy parsing and migrate Makerv3`

**Write Scope:** `systems/flux/flux/strategies/makerv3/strategy.py`, `systems/flux/flux/strategies/makerv3/publisher.py`, `systems/flux/flux/api/_payloads_signals.py`, `tests/unit_tests/flux/api/test_payloads.py`, `tests/unit_tests/flux/api/test_payload_snapshots.py`, `systems/flux/docs/makerv3.md`

**Verification Commands:**
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/strategies/makerv3/test_order_safety.py tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py -q`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/api/test_payloads.py -q -k 'quote_blockers or borrow'`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/api/test_payload_snapshots.py -q -k 'borrow_cap or quote_blockers'`

**Step 1: Implement the frozen contract**

Make implementation and docs match the contract above exactly. The alert path is actionable and cooldown-gated; this is not left open for interpretation.

**Step 2: Update state and API payload logic**

Ensure `quote_blockers` and related tradeability semantics are consistent from strategy state through API payload assembly.

**Step 3: Re-run focused tests**

Confirm the focused Makerv3 and API slices are green.

**Step 4: Commit**

```bash
git add systems/flux/flux/strategies/makerv3/strategy.py \
  systems/flux/flux/strategies/makerv3/publisher.py \
  systems/flux/flux/api/_payloads_signals.py \
  tests/unit_tests/flux/api/test_payloads.py \
  tests/unit_tests/flux/api/test_payload_snapshots.py \
  systems/flux/docs/makerv3.md
git commit -m "fix: align borrow cap alert and blocker contract"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 4: Run baseline verification and write rollback note

**Files:**
- Modify: `docs/plans/2026-03-31-strategy-platform-hardening-wave-pr0-baseline-safety-and-contract-freeze.md`
- Modify: `systems/flux/docs/makerv3.md`

**Dependencies:** `Task 3: Align alerts, state exports, and API blocker semantics`

**Write Scope:** `docs/plans/2026-03-31-strategy-platform-hardening-wave-pr0-baseline-safety-and-contract-freeze.md`, `systems/flux/docs/makerv3.md`

**Verification Commands:**
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/strategies/makerv3 -q`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/api/test_payloads.py tests/unit_tests/flux/api/test_payload_snapshots.py tests/unit_tests/flux/api/test_signals_inventory_contract.py tests/unit_tests/flux/runners/test_tokenmm_readiness.py tests/unit_tests/flux/bridge/test_handlers.py tests/unit_tests/flux/bridge/test_stream_consumer.py -q`
- `git diff --check`

**Step 1: Run the release bundle**

The Makerv3 suite plus the API/readiness/bridge contract slice must go green. This is the PR exit criterion.

**Step 2: Record rollback note**

Document that rollback is safe via whole-PR revert because no topic names, API routes, or payload field names changed.

**Step 3: Commit**

```bash
git add docs/plans/2026-03-31-strategy-platform-hardening-wave-pr0-baseline-safety-and-contract-freeze.md \
  systems/flux/docs/makerv3.md
git commit -m "docs: record pr0 verification and rollback"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

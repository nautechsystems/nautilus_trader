# Strategy Platform Hardening Wave PR6 MakerV3 Strategy Decomposition Implementation Plan

> **For the execution agent:** REQUIRED SUB-SKILL: Before implementing this plan, choose exactly one execution mode and use the matching skill: `superpowers:subagent-driven-development` for same-session execution or `superpowers:executing-plans` for a separate-session handoff.
> **Progress:** The Progress Tracker in this document is the execution source of truth and must be updated on every state change.

**Goal:** Shrink `MakerV3Strategy` into a smaller orchestration surface by extracting lifecycle, order-state bookkeeping, inventory-state assembly, and state-export orchestration helpers without changing behavior.

**Architecture:** Keep Makerv3 family-local behavior inside Makerv3, but split the current god object into collaborator modules that own coherent domains. The strategy class should become orchestration glue, not the place where every helper method lives.

**Tech Stack:** Python strategy/runtime code, Makerv3 lifecycle and state export tests, pytest.

**Context Docs:**
- Design: `docs/plans/2026-03-31-strategy-platform-hardening-wave-design.md`
- PRD: `docs/prd/2026-03-31-strategy-platform-hardening-wave.md`
- Relevant specs/runbooks: `systems/flux/docs/makerv3.md`

**Decision Summary:**
- this is family-local decomposition, not shared extraction
- collaborator modules must be named by responsibility, not by “misc” buckets
- public strategy behavior and exported payloads remain frozen

## Release Gates

| Environment | Required for merge? | Requirement |
| --- | --- | --- |
| `default-unit` | yes | lifecycle, reconciliation, runtime-param, and observability proof must pass |
| `pilot` | yes | deploy one pinned tokenmm pilot release from the PR head and validate the smoke bundle below |

## Affected Pilot Surfaces

- tokenmm lifecycle startup and shutdown
- runtime-param manager wiring
- state export and reconciliation behavior

## Pilot Deploy Units And Promotion Order

Deploy units:

- `flux.runners.tokenmm.run_node`
- `flux.api.app` if state-export payloads are served separately in pilot

Promotion order:

1. build one pinned pilot release from the exact PR head
2. deploy `flux.runners.tokenmm.run_node` and `flux.api.app` from that same release when both are in scope
3. validate lifecycle, runtime-param, reconciliation, and state-export smoke checks
4. promote the same units together only after the smoke bundle is clean

## Pilot Smoke Bundle

1. Deploy the tokenmm pilot release from the exact PR head.
2. Confirm startup, steady-state running, and shutdown remain clean.
3. Confirm runtime-param management and state export payloads remain unchanged.
4. Confirm reconciliation and pending-cancel bookkeeping behave the same during steady-state operation.

## PR-Local Rollback

- revert the whole PR
- redeploy the previous pinned pilot release
- do not mix the old strategy class with only some extracted collaborator modules

## Progress Tracker

**Source of truth:** Update this table whenever task state changes. Do not rely on memory, chat history, or TodoWrite alone.

| Task | Status | Owner | Depends On | Write Scope | Lane Branch | Worktree Path | Commit / Diff | Verification | Notes / Last Update |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Overall | not_started | unassigned | none | `systems/flux/flux/strategies/makerv3`, `tests/unit_tests/flux/strategies/makerv3`, `systems/flux/docs` | `wave/pr6-strategy-decomposition` | `.worktrees/strategy-platform-pr6` | none | not_run | Plan created |
| Task 1: Lock lifecycle and state-export behavior in tests | not_started | unassigned | none | `tests/unit_tests/flux/strategies/makerv3/test_strategy_lifecycle.py`, `tests/unit_tests/flux/strategies/makerv3/test_reconciliation.py`, `tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py`, `tests/unit_tests/flux/strategies/makerv3/test_runtime_params.py` | `wave/pr6-strategy-decomposition` | `.worktrees/strategy-platform-pr6` | none | not_run | Plan created |
| Task 2: Extract lifecycle and order-state collaborators | not_started | unassigned | Task 1: Lock lifecycle and state-export behavior in tests | `systems/flux/flux/strategies/makerv3/lifecycle.py`, `systems/flux/flux/strategies/makerv3/order_state.py`, `systems/flux/flux/strategies/makerv3/strategy.py`, `tests/unit_tests/flux/strategies/makerv3/test_strategy_lifecycle.py`, `tests/unit_tests/flux/strategies/makerv3/test_reconciliation.py` | `wave/pr6-strategy-decomposition` | `.worktrees/strategy-platform-pr6` | none | not_run | Plan created |
| Task 3: Extract inventory-state and state-export collaborators | not_started | unassigned | Task 2: Extract lifecycle and order-state collaborators | `systems/flux/flux/strategies/makerv3/inventory_state.py`, `systems/flux/flux/strategies/makerv3/state_exports.py`, `systems/flux/flux/strategies/makerv3/strategy.py`, `tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py`, `tests/unit_tests/flux/strategies/makerv3/test_runtime_params.py` | `wave/pr6-strategy-decomposition` | `.worktrees/strategy-platform-pr6` | none | not_run | Plan created |
| Task 4: Verify shrinkdown and record rollback note | not_started | unassigned | Task 3: Extract inventory-state and state-export collaborators | `systems/flux/docs/makerv3.md`, `docs/plans/2026-03-31-strategy-platform-hardening-wave-pr6-makerv3-strategy-decomposition.md` | `wave/pr6-strategy-decomposition` | `.worktrees/strategy-platform-pr6` | none | not_run | Plan created |

---

### Task 1: Lock lifecycle and state-export behavior in tests

**Files:**
- Modify: `tests/unit_tests/flux/strategies/makerv3/test_strategy_lifecycle.py`
- Modify: `tests/unit_tests/flux/strategies/makerv3/test_reconciliation.py`
- Modify: `tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py`
- Modify: `tests/unit_tests/flux/strategies/makerv3/test_runtime_params.py`

**Dependencies:** `none`

**Write Scope:** `tests/unit_tests/flux/strategies/makerv3/test_strategy_lifecycle.py`, `tests/unit_tests/flux/strategies/makerv3/test_reconciliation.py`, `tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py`, `tests/unit_tests/flux/strategies/makerv3/test_runtime_params.py`

**Verification Commands:**
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/strategies/makerv3/test_strategy_lifecycle.py tests/unit_tests/flux/strategies/makerv3/test_reconciliation.py tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py tests/unit_tests/flux/strategies/makerv3/test_runtime_params.py -q`

**Step 1: Write the failing tests**

Lock:

- startup/shutdown transitions
- order-event bookkeeping and reconciliation
- state export behavior
- runtime-param manager wiring

**Step 2: Run tests to verify they fail**

Use the focused command above.

**Step 3: Commit**

```bash
git add tests/unit_tests/flux/strategies/makerv3/test_strategy_lifecycle.py \
  tests/unit_tests/flux/strategies/makerv3/test_reconciliation.py \
  tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py \
  tests/unit_tests/flux/strategies/makerv3/test_runtime_params.py
git commit -m "test: lock makerv3 strategy decomposition behavior"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 2: Extract lifecycle and order-state collaborators

**Files:**
- Create: `systems/flux/flux/strategies/makerv3/lifecycle.py`
- Create: `systems/flux/flux/strategies/makerv3/order_state.py`
- Modify: `systems/flux/flux/strategies/makerv3/strategy.py`
- Modify: `tests/unit_tests/flux/strategies/makerv3/test_strategy_lifecycle.py`
- Modify: `tests/unit_tests/flux/strategies/makerv3/test_reconciliation.py`

**Dependencies:** `Task 1: Lock lifecycle and state-export behavior in tests`

**Write Scope:** `systems/flux/flux/strategies/makerv3/lifecycle.py`, `systems/flux/flux/strategies/makerv3/order_state.py`, `systems/flux/flux/strategies/makerv3/strategy.py`, `tests/unit_tests/flux/strategies/makerv3/test_strategy_lifecycle.py`, `tests/unit_tests/flux/strategies/makerv3/test_reconciliation.py`

**Verification Commands:**
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/strategies/makerv3/test_strategy_lifecycle.py tests/unit_tests/flux/strategies/makerv3/test_reconciliation.py -q`

**Step 1: Extract startup/shutdown wiring**

Move lifecycle-specific logic into `lifecycle.py`.

**Step 2: Extract pending-cancel and order-event bookkeeping**

Move order-state tracking into `order_state.py`.

**Step 3: Re-run focused tests**

Confirm lifecycle and reconciliation behavior is preserved.

**Step 4: Commit**

```bash
git add systems/flux/flux/strategies/makerv3/lifecycle.py \
  systems/flux/flux/strategies/makerv3/order_state.py \
  systems/flux/flux/strategies/makerv3/strategy.py \
  tests/unit_tests/flux/strategies/makerv3/test_strategy_lifecycle.py \
  tests/unit_tests/flux/strategies/makerv3/test_reconciliation.py
git commit -m "refactor: extract makerv3 lifecycle and order state"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 3: Extract inventory-state and state-export collaborators

**Files:**
- Create: `systems/flux/flux/strategies/makerv3/inventory_state.py`
- Create: `systems/flux/flux/strategies/makerv3/state_exports.py`
- Modify: `systems/flux/flux/strategies/makerv3/strategy.py`
- Modify: `tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py`
- Modify: `tests/unit_tests/flux/strategies/makerv3/test_runtime_params.py`

**Dependencies:** `Task 2: Extract lifecycle and order-state collaborators`

**Write Scope:** `systems/flux/flux/strategies/makerv3/inventory_state.py`, `systems/flux/flux/strategies/makerv3/state_exports.py`, `systems/flux/flux/strategies/makerv3/strategy.py`, `tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py`, `tests/unit_tests/flux/strategies/makerv3/test_runtime_params.py`

**Verification Commands:**
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py tests/unit_tests/flux/strategies/makerv3/test_runtime_params.py -q`

**Step 1: Extract inventory-state assembly**

Move strategy-local inventory-state gathering into `inventory_state.py`.

**Step 2: Extract state-export orchestration**

Move state payload assembly orchestration into `state_exports.py`.

**Step 3: Re-run focused tests**

Confirm exported payloads and runtime-param behavior stay unchanged.

**Step 4: Commit**

```bash
git add systems/flux/flux/strategies/makerv3/inventory_state.py \
  systems/flux/flux/strategies/makerv3/state_exports.py \
  systems/flux/flux/strategies/makerv3/strategy.py \
  tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py \
  tests/unit_tests/flux/strategies/makerv3/test_runtime_params.py
git commit -m "refactor: extract makerv3 inventory and state export collaborators"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 4: Verify shrinkdown and record rollback note

**Files:**
- Modify: `systems/flux/docs/makerv3.md`
- Modify: `docs/plans/2026-03-31-strategy-platform-hardening-wave-pr6-makerv3-strategy-decomposition.md`

**Dependencies:** `Task 3: Extract inventory-state and state-export collaborators`

**Write Scope:** `systems/flux/docs/makerv3.md`, `docs/plans/2026-03-31-strategy-platform-hardening-wave-pr6-makerv3-strategy-decomposition.md`

**Verification Commands:**
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/strategies/makerv3/test_strategy_lifecycle.py tests/unit_tests/flux/strategies/makerv3/test_reconciliation.py tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py tests/unit_tests/flux/strategies/makerv3/test_runtime_params.py -q`
- `git diff --check`

**Step 1: Run the combined verification bundle**

The lifecycle, reconciliation, observability, and runtime-param slices must pass together.

**Step 2: Update docs and rollback note**

Document the new Makerv3 internal collaborator layout and that public behavior remained frozen.

**Step 3: Commit**

```bash
git add systems/flux/docs/makerv3.md \
  docs/plans/2026-03-31-strategy-platform-hardening-wave-pr6-makerv3-strategy-decomposition.md
git commit -m "docs: record pr6 verification and rollback"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

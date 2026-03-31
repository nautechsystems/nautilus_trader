# Strategy Platform Hardening Wave PR5 MakerV3 Quote Pipeline Split Implementation Plan

> **For the execution agent:** REQUIRED SUB-SKILL: Before implementing this plan, choose exactly one execution mode and use the matching skill: `superpowers:subagent-driven-development` for same-session execution or `superpowers:executing-plans` for a separate-session handoff.
> **Progress:** The Progress Tracker in this document is the execution source of truth and must be updated on every state change.

**Goal:** Split the Makerv3 quote-refresh hot path into explicit collaborators without changing quoting behavior, operator payloads, or safety gates.

**Architecture:** Break `quote_engine.refresh_quotes` into family-local collaborator modules for preflight/gating, target assembly, action execution, and telemetry recording. Preserve all existing risk, stale-data, pending-cancel, and observability behavior while making the hot path auditable and easier to test directly.

**Tech Stack:** Python strategy/runtime code, Makerv3 quote engine, pytest.

**Context Docs:**
- Design: `docs/plans/2026-03-31-strategy-platform-hardening-wave-design.md`
- PRD: `docs/prd/2026-03-31-strategy-platform-hardening-wave.md`
- Relevant specs/runbooks: `systems/flux/docs/makerv3.md`, `docs/plans/2026-03-26-shared-deque-quote-stack-design.md`

**Decision Summary:**
- this is a structural PR only
- no external contract changes are allowed
- new collaborator modules stay family-local because this split is not a new shared boundary

## Release Gates

| Environment | Required for merge? | Requirement |
| --- | --- | --- |
| `default-unit` | yes | quote-engine, order-intent, and observability proof must pass |
| `pilot` | yes | deploy one pinned tokenmm pilot release from the PR head and validate the smoke bundle below |

## Affected Pilot Surfaces

- tokenmm quote lifecycle and quote refresh behavior
- order-intent exports and quote telemetry
- Makerv3 operator state and alert surfaces that reflect quote progress

## Pilot Deploy Units And Promotion Order

Deploy units:

- `flux.runners.tokenmm.run_node`
- `flux.api.app` if operator payloads or order-intent surfaces are served separately in pilot

Promotion order:

1. build one pinned pilot release from the exact PR head
2. deploy `flux.runners.tokenmm.run_node` and `flux.api.app` from that same release when both are in scope
3. validate quote lifecycle, order-intent, and telemetry smoke checks
4. promote the same units together only after the smoke bundle is clean

## Pilot Smoke Bundle

1. Deploy the tokenmm pilot release from the exact PR head.
2. Confirm quote refresh continues to place, amend, and cancel quotes in the same steady-state scenarios.
3. Confirm quote-cycle, order-intent, and observability payloads remain unchanged.
4. Confirm no new family-shared dependency is introduced by the split.

## PR-Local Rollback

- revert the whole PR
- redeploy the previous pinned pilot release
- do not mix old quote-engine code with partially extracted collaborator modules

## Progress Tracker

**Source of truth:** Update this table whenever task state changes. Do not rely on memory, chat history, or TodoWrite alone.

| Task | Status | Owner | Depends On | Write Scope | Lane Branch | Worktree Path | Commit / Diff | Verification | Notes / Last Update |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Overall | not_started | unassigned | none | `systems/flux/flux/strategies/makerv3`, `tests/unit_tests/flux/strategies/makerv3`, `systems/flux/docs` | `wave/pr5-quote-pipeline-split` | `.worktrees/strategy-platform-pr5` | none | not_run | Plan created |
| Task 1: Lock quote-pipeline behavior with focused regression tests | not_started | unassigned | none | `tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py`, `tests/unit_tests/flux/strategies/makerv3/test_order_intent_exports.py`, `tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py` | `wave/pr5-quote-pipeline-split` | `.worktrees/strategy-platform-pr5` | none | not_run | Plan created |
| Task 2: Extract preflight and target-building collaborators | not_started | unassigned | Task 1: Lock quote-pipeline behavior with focused regression tests | `systems/flux/flux/strategies/makerv3/quote_preflight.py`, `systems/flux/flux/strategies/makerv3/quote_targets.py`, `systems/flux/flux/strategies/makerv3/quote_engine.py`, `tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py` | `wave/pr5-quote-pipeline-split` | `.worktrees/strategy-platform-pr5` | none | not_run | Plan created |
| Task 3: Extract action-execution and telemetry collaborators | not_started | unassigned | Task 2: Extract preflight and target-building collaborators | `systems/flux/flux/strategies/makerv3/quote_actions.py`, `systems/flux/flux/strategies/makerv3/quote_telemetry.py`, `systems/flux/flux/strategies/makerv3/quote_engine.py`, `tests/unit_tests/flux/strategies/makerv3/test_order_intent_exports.py`, `tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py` | `wave/pr5-quote-pipeline-split` | `.worktrees/strategy-platform-pr5` | none | not_run | Plan created |
| Task 4: Verify behavior preservation and record rollback note | not_started | unassigned | Task 3: Extract action-execution and telemetry collaborators | `systems/flux/docs/makerv3.md`, `docs/plans/2026-03-31-strategy-platform-hardening-wave-pr5-makerv3-quote-pipeline-split.md` | `wave/pr5-quote-pipeline-split` | `.worktrees/strategy-platform-pr5` | none | not_run | Plan created |

---

### Task 1: Lock quote-pipeline behavior with focused regression tests

**Files:**
- Modify: `tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py`
- Modify: `tests/unit_tests/flux/strategies/makerv3/test_order_intent_exports.py`
- Modify: `tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py`

**Dependencies:** `none`

**Write Scope:** `tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py`, `tests/unit_tests/flux/strategies/makerv3/test_order_intent_exports.py`, `tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py`

**Verification Commands:**
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py tests/unit_tests/flux/strategies/makerv3/test_order_intent_exports.py tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py -q`

**Step 1: Write the failing tests**

Lock:

- stale-data blocks
- pending-cancel blocking
- action ordering
- quote-cycle and order-intent payload content

**Step 2: Run tests to verify they fail**

Use the focused command above.

**Step 3: Commit**

```bash
git add tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py \
  tests/unit_tests/flux/strategies/makerv3/test_order_intent_exports.py \
  tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py
git commit -m "test: lock makerv3 quote pipeline behavior"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 2: Extract preflight and target-building collaborators

**Files:**
- Create: `systems/flux/flux/strategies/makerv3/quote_preflight.py`
- Create: `systems/flux/flux/strategies/makerv3/quote_targets.py`
- Modify: `systems/flux/flux/strategies/makerv3/quote_engine.py`
- Modify: `tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py`

**Dependencies:** `Task 1: Lock quote-pipeline behavior with focused regression tests`

**Write Scope:** `systems/flux/flux/strategies/makerv3/quote_preflight.py`, `systems/flux/flux/strategies/makerv3/quote_targets.py`, `systems/flux/flux/strategies/makerv3/quote_engine.py`, `tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py`

**Verification Commands:**
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py -q`

**Step 1: Extract the preflight gate logic**

Move stale-data, startup cleanup, and pending-cancel admission logic into `quote_preflight.py`.

**Step 2: Extract target assembly**

Move desired target and side-planning preparation into `quote_targets.py`.

**Step 3: Re-run focused tests**

Confirm preflight and target behavior did not change.

**Step 4: Commit**

```bash
git add systems/flux/flux/strategies/makerv3/quote_preflight.py \
  systems/flux/flux/strategies/makerv3/quote_targets.py \
  systems/flux/flux/strategies/makerv3/quote_engine.py \
  tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py
git commit -m "refactor: extract makerv3 quote preflight and targets"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 3: Extract action-execution and telemetry collaborators

**Files:**
- Create: `systems/flux/flux/strategies/makerv3/quote_actions.py`
- Create: `systems/flux/flux/strategies/makerv3/quote_telemetry.py`
- Modify: `systems/flux/flux/strategies/makerv3/quote_engine.py`
- Modify: `tests/unit_tests/flux/strategies/makerv3/test_order_intent_exports.py`
- Modify: `tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py`

**Dependencies:** `Task 2: Extract preflight and target-building collaborators`

**Write Scope:** `systems/flux/flux/strategies/makerv3/quote_actions.py`, `systems/flux/flux/strategies/makerv3/quote_telemetry.py`, `systems/flux/flux/strategies/makerv3/quote_engine.py`, `tests/unit_tests/flux/strategies/makerv3/test_order_intent_exports.py`, `tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py`

**Verification Commands:**
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py tests/unit_tests/flux/strategies/makerv3/test_order_intent_exports.py tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py -q`

**Step 1: Extract order/cancel execution sequencing**

Move action execution logic to `quote_actions.py`.

**Step 2: Extract telemetry assembly**

Move quote-cycle diagnostics and related envelope helpers to `quote_telemetry.py`.

**Step 3: Re-run the combined bundle**

All focused quote-engine, order-intent, and observability tests must pass together.

**Step 4: Commit**

```bash
git add systems/flux/flux/strategies/makerv3/quote_actions.py \
  systems/flux/flux/strategies/makerv3/quote_telemetry.py \
  systems/flux/flux/strategies/makerv3/quote_engine.py \
  tests/unit_tests/flux/strategies/makerv3/test_order_intent_exports.py \
  tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py
git commit -m "refactor: extract makerv3 quote actions and telemetry"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 4: Verify behavior preservation and record rollback note

**Files:**
- Modify: `systems/flux/docs/makerv3.md`
- Modify: `docs/plans/2026-03-31-strategy-platform-hardening-wave-pr5-makerv3-quote-pipeline-split.md`

**Dependencies:** `Task 3: Extract action-execution and telemetry collaborators`

**Write Scope:** `systems/flux/docs/makerv3.md`, `docs/plans/2026-03-31-strategy-platform-hardening-wave-pr5-makerv3-quote-pipeline-split.md`

**Verification Commands:**
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py tests/unit_tests/flux/strategies/makerv3/test_order_intent_exports.py tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py -q`
- `git diff --check`

**Step 1: Run the focused bundle**

This PR is complete only when the entire focused quote-pipeline bundle is green.

**Step 2: Update docs and rollback note**

Document the new module layout and explicitly state that this was a structural split with no contract changes.

**Step 3: Commit**

```bash
git add systems/flux/docs/makerv3.md \
  docs/plans/2026-03-31-strategy-platform-hardening-wave-pr5-makerv3-quote-pipeline-split.md
git commit -m "docs: record pr5 verification and rollback"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

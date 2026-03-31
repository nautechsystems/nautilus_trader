# Strategy Platform Hardening Wave PR7 Docstrings Runbooks And Cleanup Implementation Plan

> **For the execution agent:** REQUIRED SUB-SKILL: Before implementing this plan, choose exactly one execution mode and use the matching skill: `superpowers:subagent-driven-development` for same-session execution or `superpowers:executing-plans` for a separate-session handoff.
> **Progress:** The Progress Tracker in this document is the execution source of truth and must be updated on every state change.

**Goal:** Finish the wave with residual documentation, docstring, runbook, and cleanup work after the code boundaries are stable, without deferring earlier PR contract coverage into this final PR.

**Architecture:** Use this PR only for residual cleanup that becomes clearer after the wave lands: platform docs, runbooks, docstrings on newly public shared/common modules, and small invariant tests for PR5/PR6-created modules. This PR is not allowed to backfill contract tests or rollback notes that earlier PRs should have shipped.

**Tech Stack:** Markdown docs, Python docstrings, docs tests, lint, pytest.

**Context Docs:**
- Design: `docs/plans/2026-03-31-strategy-platform-hardening-wave-design.md`
- PRD: `docs/prd/2026-03-31-strategy-platform-hardening-wave.md`
- Relevant specs/runbooks: `systems/flux/docs/makerv3.md`, `docs/runbooks/tokenmm-makerv3-bounded-convergence-rollout.md`

**Decision Summary:**
- earlier PRs own their own contract tests and rollback notes
- this PR is for residual narrative/documentation closeout only
- any new invariants added here are for modules created late in the wave, not for deferred shared extraction coverage

## Release Gates

| Environment | Required for merge? | Requirement |
| --- | --- | --- |
| `default-unit` | yes | docs tests, residual invariants, and docstring lint must pass |

This PR does not require a pilot release gate unless it unexpectedly stops being docs-and-invariants-only. If code behavior or operator contracts change, the work belongs in an earlier PR instead.

## Progress Tracker

**Source of truth:** Update this table whenever task state changes. Do not rely on memory, chat history, or TodoWrite alone.

| Task | Status | Owner | Depends On | Write Scope | Lane Branch | Worktree Path | Commit / Diff | Verification | Notes / Last Update |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Overall | not_started | unassigned | none | `systems/flux/docs`, `docs/runbooks`, `docs/plans`, `systems/flux/flux/common`, `systems/flux/flux/strategies/shared`, `systems/flux/flux/strategies/makerv3`, `tests/unit_tests/docs`, `tests/unit_tests/flux/strategies/shared`, `tests/unit_tests/flux/strategies/makerv3` | `wave/pr7-docs-cleanup` | `.worktrees/strategy-platform-pr7` | none | not_run | Plan created |
| Task 1: Audit residual docstring and docs gaps from the wave | not_started | unassigned | none | `systems/flux/docs`, `docs/runbooks`, `docs/plans`, `systems/flux/flux/common`, `systems/flux/flux/strategies/shared`, `systems/flux/flux/strategies/makerv3` | `wave/pr7-docs-cleanup` | `.worktrees/strategy-platform-pr7` | none | not_run | Plan created |
| Task 2: Update platform docs and operator runbooks | not_started | unassigned | Task 1: Audit residual docstring and docs gaps from the wave | `systems/flux/docs/makerv3.md`, `systems/flux/docs/strategy_platform.md`, `docs/runbooks/tokenmm-makerv3-bounded-convergence-rollout.md`, `docs/plans/2026-03-31-strategy-platform-hardening-wave-review-packet.md` | `wave/pr7-docs-cleanup` | `.worktrees/strategy-platform-pr7` | none | not_run | Plan created |
| Task 3: Add residual public docstrings and direct invariants for late-created modules | not_started | unassigned | Task 2: Update platform docs and operator runbooks | `systems/flux/flux/common/*.py`, `systems/flux/flux/strategies/shared/*.py`, `systems/flux/flux/strategies/makerv3/*.py`, `tests/unit_tests/flux/strategies/shared`, `tests/unit_tests/flux/strategies/makerv3` | `wave/pr7-docs-cleanup` | `.worktrees/strategy-platform-pr7` | none | not_run | Plan created |
| Task 4: Run docs and lint verification, then record rollback note | not_started | unassigned | Task 3: Add residual public docstrings and direct invariants for late-created modules | `docs/plans/2026-03-31-strategy-platform-hardening-wave-pr7-docstrings-runbooks-and-cleanup.md` | `wave/pr7-docs-cleanup` | `.worktrees/strategy-platform-pr7` | none | not_run | Plan created |

---

### Task 1: Audit residual docstring and docs gaps from the wave

**Files:**
- Modify: `docs/plans/2026-03-31-strategy-platform-hardening-wave-pr7-docstrings-runbooks-and-cleanup.md`

**Dependencies:** `none`

**Write Scope:** `docs/plans/2026-03-31-strategy-platform-hardening-wave-pr7-docstrings-runbooks-and-cleanup.md`

**Verification Commands:**
- `rg -n "^def |^class " systems/flux/flux/common systems/flux/flux/strategies/shared systems/flux/flux/strategies/makerv3`
- `rg -n "TODO|TBD|legacy contract|update docs" systems/flux/docs docs/runbooks docs/plans`

**Step 1: Enumerate the residual gaps**

List modules and doc surfaces that still need cleanup after PR0-PR6.

**Step 2: Confirm the gaps are truly residual**

Do not use this PR to smuggle in missed contract work from earlier PRs.

**Step 3: Commit**

```bash
git add docs/plans/2026-03-31-strategy-platform-hardening-wave-pr7-docstrings-runbooks-and-cleanup.md
git commit -m "docs: audit residual wave cleanup scope"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 2: Update platform docs and operator runbooks

**Files:**
- Modify: `systems/flux/docs/makerv3.md`
- Create: `systems/flux/docs/strategy_platform.md`
- Modify: `docs/runbooks/tokenmm-makerv3-bounded-convergence-rollout.md`
- Modify: `docs/plans/2026-03-31-strategy-platform-hardening-wave-review-packet.md`

**Dependencies:** `Task 1: Audit residual docstring and docs gaps from the wave`

**Write Scope:** `systems/flux/docs/makerv3.md`, `systems/flux/docs/strategy_platform.md`, `docs/runbooks/tokenmm-makerv3-bounded-convergence-rollout.md`, `docs/plans/2026-03-31-strategy-platform-hardening-wave-review-packet.md`

**Verification Commands:**
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/docs -q`

**Step 1: Update the canonical docs**

Describe the final platform layering and the new Makerv3 internal module layout.

**Step 2: Update runbooks**

Make operator guidance match the final ownership and observability model.

**Step 3: Commit**

```bash
git add systems/flux/docs/makerv3.md \
  systems/flux/docs/strategy_platform.md \
  docs/runbooks/tokenmm-makerv3-bounded-convergence-rollout.md \
  docs/plans/2026-03-31-strategy-platform-hardening-wave-review-packet.md
git commit -m "docs: update platform and operator documentation"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 3: Add residual public docstrings and direct invariants for late-created modules

**Files:**
- Modify: `systems/flux/flux/common/*.py`
- Modify: `systems/flux/flux/strategies/shared/*.py`
- Modify: `systems/flux/flux/strategies/makerv3/*.py`
- Modify: `tests/unit_tests/flux/strategies/shared/*`
- Modify: `tests/unit_tests/flux/strategies/makerv3/*`

**Dependencies:** `Task 2: Update platform docs and operator runbooks`

**Write Scope:** `systems/flux/flux/common/*.py`, `systems/flux/flux/strategies/shared/*.py`, `systems/flux/flux/strategies/makerv3/*.py`, `tests/unit_tests/flux/strategies/shared/*`, `tests/unit_tests/flux/strategies/makerv3/*`

**Verification Commands:**
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/strategies/shared tests/unit_tests/flux/strategies/makerv3 -q`
- `ruff check --select D systems/flux/flux/common systems/flux/flux/strategies/shared systems/flux/flux/strategies/makerv3`

**Step 1: Add missing public docstrings**

Focus on public modules and contracts created late in the wave.

**Step 2: Add residual direct invariants**

Only add direct tests for late-created public modules that do not already have strong focused coverage.

**Step 3: Commit**

```bash
git add systems/flux/flux/common \
  systems/flux/flux/strategies/shared \
  systems/flux/flux/strategies/makerv3 \
  tests/unit_tests/flux/strategies/shared \
  tests/unit_tests/flux/strategies/makerv3
git commit -m "docs: complete residual docstrings and invariants"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 4: Run docs and lint verification, then record rollback note

**Files:**
- Modify: `docs/plans/2026-03-31-strategy-platform-hardening-wave-pr7-docstrings-runbooks-and-cleanup.md`

**Dependencies:** `Task 3: Add residual public docstrings and direct invariants for late-created modules`

**Write Scope:** `docs/plans/2026-03-31-strategy-platform-hardening-wave-pr7-docstrings-runbooks-and-cleanup.md`

**Verification Commands:**
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/docs tests/unit_tests/flux/strategies/shared tests/unit_tests/flux/strategies/makerv3 -q`
- `ruff check --select D systems/flux/flux/common systems/flux/flux/strategies/shared systems/flux/flux/strategies/makerv3`
- `git diff --check`

**Step 1: Run the closeout bundle**

Docs tests, residual invariants, and docstring lint must all pass.

**Step 2: Record rollback note**

State explicitly that this PR is documentation and residual invariants only, so whole-PR revert is straightforward.

**Step 3: Commit**

```bash
git add docs/plans/2026-03-31-strategy-platform-hardening-wave-pr7-docstrings-runbooks-and-cleanup.md
git commit -m "docs: record pr7 verification and rollback"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

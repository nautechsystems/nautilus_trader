# TokenMM Rollout Blockers And PR Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.
> **Progress:** The Progress Tracker in this document is the execution source of truth and must be updated on every state change.

**Goal:** Make the TokenMM telemetry rollout branch self-checking, documented, and ready to ship as a PR.

**Architecture:** Add a deploy preflight that validates built SPA assets and native extension exports before systemd envs are rendered, then update rollout docs/tests so operators follow the same path the host needs in production. Keep Jupyter localhost-only and document the direct notebook URL instead of adding a fragile API proxy.

**Tech Stack:** Python 3.12, Flask runner wiring, systemd env generation, pytest, markdown runbooks.

## Progress Tracker

**Source of truth:** Update this table whenever task state changes. Do not rely on memory, chat history, or TodoWrite alone.

| Task | Status | Owner | Notes / Last Update |
| --- | --- | --- | --- |
| Overall | in_progress | main | Preflight, docs, and rollout contract are implemented; verification green; commit and PR packaging pending |
| Task 1: Add Rollout Preflight | completed | main | Added checkout preflight for built SPA assets and required native exports; installer now runs it before writing envs |
| Task 2: Cover Preflight And Notebook Addressing In Tests | completed | main | Added rollout preflight tests and expanded stack contract for checkout-pinned runtime paths and direct Jupyter address |
| Task 3: Update Rollout Docs To Match Host Reality | completed | main | Deploy README and runbooks now document checkout `.venv` preflight, localhost Jupyter, and TokenMM env overrides |
| Task 4: Verify, Commit, And Open PR | in_progress | main | `bash -n` clean, real preflight OK, and TokenMM regression slice is `231 passed`; commit/push/PR still pending |

---

### Task 1: Add Rollout Preflight

**Files:**
- Create: `ops/scripts/deploy/tokenmm_rollout_preflight.py`
- Modify: `ops/scripts/deploy/install_tokenmm_systemd.sh`
- Modify: `docs/plans/2026-03-09-tokenmm-rollout-blockers-and-pr.md`

**Step 1: Write the failing test**

Add contract coverage that requires the installer to invoke a rollout preflight and requires the preflight source to check built Fluxboard/Pulse assets plus native extension exports for enabled venues.

**Step 2: Run test to verify it fails**

Run: `PYTHONPATH=/home/ubuntu/nautilus_trader/.worktrees/tokenmm-telemetry-go-prod:/home/ubuntu/nautilus_trader/.worktrees/tokenmm-telemetry-go-prod/systems/flux python3.12 -m pytest tests/unit_tests/examples/strategies/test_tokenmm_stack_contract.py -q`
Expected: FAIL on missing preflight coverage.

**Step 3: Write minimal implementation**

Add a small Python preflight script that:
- resolves repo-root paths,
- verifies `fluxboard/dist/index.html` and `pulse-ui/dist/index.html`,
- loads enabled TokenMM strategy venue requirements,
- imports `nautilus_trader.core.nautilus_pyo3`,
- checks required venue exports such as `BitgetEnvironment`,
- exits non-zero with actionable messages.

Update the installer to call the preflight before rendering env files.

**Step 4: Run test to verify it passes**

Run the same pytest command and confirm it passes.

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 2: Cover Preflight And Notebook Addressing In Tests

**Files:**
- Modify: `tests/unit_tests/examples/strategies/test_tokenmm_stack_contract.py`

**Step 1: Write the failing test**

Extend the stack contract to require notebook docs to advertise the direct Jupyter URL and to require the new preflight script surface.

**Step 2: Run test to verify it fails**

Run the same targeted pytest slice and confirm the new assertions fail.

**Step 3: Write minimal implementation**

Only add the production code and docs needed to satisfy the new contract.

**Step 4: Run test to verify it passes**

Re-run the targeted pytest slice and confirm it is green.

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 3: Update Rollout Docs To Match Host Reality

**Files:**
- Modify: `deploy/tokenmm/README.md`
- Modify: `fluxboard/docs/tokenmm_runbook.md`
- Modify: `deploy/tokenmm/TELEMETRY_RDS_RUNBOOK.md`

**Step 1: Write the failing test**

Require docs to include runtime build commands, the preflight command, and the direct localhost JupyterLab address.

**Step 2: Run test to verify it fails**

Run the targeted stack contract slice and confirm docs are missing the new text.

**Step 3: Write minimal implementation**

Document:
- `make build`
- `pnpm --dir fluxboard build`
- `pnpm --dir pulse-ui build`
- `python3 ops/scripts/deploy/tokenmm_rollout_preflight.py`
- `http://127.0.0.1:8888/lab`

**Step 4: Run test to verify it passes**

Re-run the targeted stack contract slice and confirm it is green.

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 4: Verify, Commit, And Open PR

**Files:**
- Modify: `docs/plans/2026-03-09-tokenmm-rollout-blockers-and-pr.md`

**Step 1: Run full relevant verification**

Run the TokenMM regression subset plus any targeted preflight tests.

**Step 2: Inspect git diff and status**

Review changed files for unintended edits and summarize the rollout delta.

**Step 3: Commit**

Create a focused commit for the rollout blocker fixes.

**Step 4: Open the PR**

Push the branch and open a PR with a concise rollout-oriented description.

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

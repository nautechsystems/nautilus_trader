# Strategy Platform Hardening Wave PR2 Shared Account Projection Ownership Implementation Plan

> **For the execution agent:** REQUIRED SUB-SKILL: Before implementing this plan, choose exactly one execution mode and use the matching skill: `superpowers:subagent-driven-development` for same-session execution or `superpowers:executing-plans` for a separate-session handoff.
> **Progress:** The Progress Tracker in this document is the execution source of truth and must be updated on every state change.

**Goal:** Move shared-account projection row-selection helpers out of strategy-owned shared modules and into the common/read-model layer, move the runner-owned IBKR reference-balance provider into `flux.runners.shared`, and then migrate Makerv3, Makerv4, runner, and API consumers without changing behavior.

**Architecture:** Treat shared-account projection lookup as a read-model concern, not a strategy-platform concern. Create a common ownership module for account-projection position helpers, move the IBKR reference-balance provider and cache into `flux.runners.shared`, migrate Makerv3, Makerv4, runner, and API consumers onto those new owners, and delete the strategy-shared copies in the same PR.

**Tech Stack:** Python common/read-model code, Makerv3 and Makerv4 strategy code, profile-account tests, pytest.

**Context Docs:**
- Design: `docs/plans/2026-03-31-strategy-platform-hardening-wave-design.md`
- PRD: `docs/prd/2026-03-31-strategy-platform-hardening-wave.md`
- Relevant specs/runbooks: `docs/plans/2026-03-28-account-scoped-execution-controller-design.md`, `tests/unit_tests/flux/strategies/shared/test_account_projection_positions.py`

**Decision Summary:**
- projection row selection belongs outside `strategies/shared`
- this PR changes ownership only, not read-model semantics
- both Makerv3 and Makerv4 must migrate in the same PR
- the runner-owned IBKR reference-balance provider must stop living under `strategies/shared.equities_arb`
- no strategy-layer `reference_balances` re-export may survive this PR

## Release Gates

| Environment | Required for merge? | Requirement |
| --- | --- | --- |
| `default-unit` | yes | common/read-model, Makerv3, API, and runner proof must pass |
| `ibkr-unit` | yes | `python -c "import ibapi"` must succeed and Makerv4 proof must pass |
| `pilot` | yes | deploy one pinned pilot release from the PR head and validate the smoke bundle below |

## Affected Pilot Surfaces

- shared-account and profile-account consumers in Makerv3 and Makerv4
- runner profile-account surfaces
- API inventory and balances views that consume projection truth
- equities runner bootstrap and the moved IBKR reference-balance provider path

## Surface Contract Map

- `profile-account consumers`: `systems/flux/flux/runners/shared/profile_accounts.py` outputs and the balances/profile rows they feed
- `shared-account views`: strategy inventory state and projection-derived rows consumed by `/api/v1/signals`
- `inventory-facing API views`: `/api/v1/signals` and `/api/v1/balances` payloads that depend on shared-account row selection
- `reference-balance provider`: the IBKR snapshot provider currently rooted under `strategies/shared.equities_arb`

## Pilot Stacks To Move Together

- `tokenmm`
- `equities`
- `flux-api` if it is deployed as a separate pilot stack

## Pilot Deploy Units And Promotion Order

Deploy units:

- `flux.runners.tokenmm.run_node`
- `flux.runners.equities.run_node`
- `flux.api.app`

Promotion order:

1. build one pinned pilot release from the exact PR head
2. deploy `flux.runners.tokenmm.run_node`, `flux.runners.equities.run_node`, and `flux.api.app` from that same release
3. validate projection-derived views and runner-owned reference-balance bootstrap
4. promote the same units together only after the smoke bundle is clean

## Pilot Smoke Bundle

1. Deploy the pilot release from the exact PR head.
2. Confirm profile-account and shared-account views still resolve the same rows for live strategy surfaces.
3. Confirm tokenmm and Makerv4 inventory-facing views continue to publish the same projection-derived state.
4. Confirm the equities runner still initializes the moved runner-owned IBKR reference-balance provider and serves the expected balance source without importing deleted strategy-layer paths.
5. Confirm no strategy still imports the deleted `strategies/shared/account_projection_positions.py` path, and no runtime path still imports the deleted strategy-layer `reference_balances` modules.

## PR-Local Rollback

- revert the whole PR
- redeploy the previous pinned pilot release
- do not leave common projection helpers live while reverting only one consumer

## Progress Tracker

**Source of truth:** Update this table whenever task state changes. Do not rely on memory, chat history, or TodoWrite alone.

| Task | Status | Owner | Depends On | Write Scope | Lane Branch | Worktree Path | Commit / Diff | Verification | Notes / Last Update |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Overall | not_started | unassigned | none | `systems/flux/flux/common`, `systems/flux/flux/strategies/makerv3`, `systems/flux/flux/strategies/makerv4`, `systems/flux/flux/strategies/shared`, `tests/unit_tests/flux/common`, `tests/unit_tests/flux/strategies/makerv3`, `tests/unit_tests/flux/strategies/makerv4` | `wave/pr2-projection-ownership` | `.worktrees/strategy-platform-pr2` | none | not_run | Plan created |
| Task 1: Lock ownership and read-model behavior in tests | not_started | unassigned | none | `tests/unit_tests/flux/test_architecture_boundaries.py`, `tests/unit_tests/flux/strategies/shared/test_account_projection_positions.py`, `tests/unit_tests/flux/strategies/shared/test_equities_arb_core.py`, `tests/unit_tests/flux/strategies/makerv3/test_inventory.py`, `tests/unit_tests/flux/strategies/makerv4/test_inventory_contract.py`, `tests/unit_tests/flux/strategies/makerv4/test_reference_balances.py`, `tests/unit_tests/flux/runners/shared/test_profile_accounts.py`, `tests/unit_tests/flux/api/test_signals_inventory_contract.py`, `tests/unit_tests/flux/api/test_payloads.py` | `wave/pr2-projection-ownership` | `.worktrees/strategy-platform-pr2` | none | not_run | Plan created |
| Task 2: Create common account-projection position helper and runner-owned reference-balance provider | not_started | unassigned | Task 1: Lock ownership and read-model behavior in tests | `systems/flux/flux/common/account_projection_positions.py`, `systems/flux/flux/runners/shared/reference_balances.py`, `tests/unit_tests/flux/common/test_account_projection_positions.py`, `tests/unit_tests/flux/runners/shared/test_reference_balances.py`, `systems/flux/flux/strategies/shared/account_projection_positions.py`, `systems/flux/flux/strategies/shared/equities_arb/reference_balances.py` | `wave/pr2-projection-ownership` | `.worktrees/strategy-platform-pr2` | none | not_run | Plan created |
| Task 3: Migrate Makerv3, Makerv4, runner, and API consumers and delete the strategy-shared paths | not_started | unassigned | Task 2: Create common account-projection position helper and runner-owned reference-balance provider | `systems/flux/flux/strategies/makerv3/strategy.py`, `systems/flux/flux/strategies/makerv4/strategy.py`, `systems/flux/flux/runners/shared/profile_accounts.py`, `systems/flux/flux/runners/equities/run_node.py`, `systems/flux/flux/strategies/shared/equities_arb/__init__.py`, `systems/flux/flux/strategies/makerv4/reference_balances.py`, `tests/unit_tests/flux/strategies/shared/test_equities_arb_core.py`, `tests/unit_tests/flux/strategies/makerv3/test_inventory.py`, `tests/unit_tests/flux/strategies/makerv4/test_inventory_contract.py`, `tests/unit_tests/flux/strategies/makerv4/test_reference_balances.py`, `tests/unit_tests/flux/runners/shared/test_profile_accounts.py`, `tests/unit_tests/flux/api/test_signals_inventory_contract.py` | `wave/pr2-projection-ownership` | `.worktrees/strategy-platform-pr2` | none | not_run | Plan created |
| Task 4: Verify common/runner ownership and write rollback note | not_started | unassigned | Task 3: Migrate Makerv3, Makerv4, runner, and API consumers and delete the strategy-shared paths | `docs/plans/2026-03-31-strategy-platform-hardening-wave-pr2-shared-account-projection-ownership.md` | `wave/pr2-projection-ownership` | `.worktrees/strategy-platform-pr2` | none | not_run | Plan created |

---

### Task 1: Lock ownership and read-model behavior in tests

**Files:**
- Modify: `tests/unit_tests/flux/strategies/shared/test_account_projection_positions.py`
- Modify: `tests/unit_tests/flux/strategies/shared/test_equities_arb_core.py`
- Modify: `tests/unit_tests/flux/strategies/makerv3/test_inventory.py`
- Modify: `tests/unit_tests/flux/strategies/makerv4/test_inventory_contract.py`
- Modify: `tests/unit_tests/flux/strategies/makerv4/test_reference_balances.py`
- Modify: `tests/unit_tests/flux/runners/shared/test_profile_accounts.py`
- Modify: `tests/unit_tests/flux/api/test_signals_inventory_contract.py`
- Modify: `tests/unit_tests/flux/api/test_payloads.py`
- Modify: `tests/unit_tests/flux/test_architecture_boundaries.py`

**Dependencies:** `none`

**Write Scope:** `tests/unit_tests/flux/test_architecture_boundaries.py`, `tests/unit_tests/flux/strategies/shared/test_account_projection_positions.py`, `tests/unit_tests/flux/strategies/shared/test_equities_arb_core.py`, `tests/unit_tests/flux/strategies/makerv3/test_inventory.py`, `tests/unit_tests/flux/strategies/makerv4/test_inventory_contract.py`, `tests/unit_tests/flux/strategies/makerv4/test_reference_balances.py`, `tests/unit_tests/flux/runners/shared/test_profile_accounts.py`, `tests/unit_tests/flux/api/test_signals_inventory_contract.py`, `tests/unit_tests/flux/api/test_payloads.py`

**Verification Commands:**
- `python -c "import ibapi"`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/test_architecture_boundaries.py -q -k 'reference_balances or account_projection or runner_imports'`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/strategies/shared/test_account_projection_positions.py tests/unit_tests/flux/strategies/shared/test_equities_arb_core.py tests/unit_tests/flux/strategies/makerv3/test_inventory.py tests/unit_tests/flux/runners/shared/test_profile_accounts.py tests/unit_tests/flux/api/test_signals_inventory_contract.py tests/unit_tests/flux/api/test_payloads.py -q`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/strategies/makerv4/test_inventory_contract.py tests/unit_tests/flux/strategies/makerv4/test_reference_balances.py -q`

**Step 1: Write the failing tests**

Pin:

- row-selection semantics
- strategy consumer behavior
- runner profile-account and reference-balance behavior
- exact API inventory-facing surfaces that consume these rows, including `/api/v1/balances`
- the fact that ownership is changing, not the returned payload contract
- the architecture rule that strategies may not import the runner-owned reference-balance provider directly

**Step 2: Run tests to verify they fail**

Run the shared/common/Makerv3 slice in `default-unit` and the Makerv4 slice in `ibkr-unit`.

**Step 3: Commit**

```bash
git add tests/unit_tests/flux/strategies/shared/test_account_projection_positions.py \
  tests/unit_tests/flux/strategies/shared/test_equities_arb_core.py \
  tests/unit_tests/flux/strategies/makerv3/test_inventory.py \
  tests/unit_tests/flux/strategies/makerv4/test_inventory_contract.py \
  tests/unit_tests/flux/strategies/makerv4/test_reference_balances.py \
  tests/unit_tests/flux/runners/shared/test_profile_accounts.py \
  tests/unit_tests/flux/api/test_signals_inventory_contract.py \
  tests/unit_tests/flux/api/test_payloads.py \
  tests/unit_tests/flux/test_architecture_boundaries.py
git commit -m "test: lock account projection ownership contract"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 2: Create common account-projection position helper and runner-owned reference-balance provider

**Files:**
- Create: `systems/flux/flux/common/account_projection_positions.py`
- Create: `systems/flux/flux/runners/shared/reference_balances.py`
- Create: `tests/unit_tests/flux/common/test_account_projection_positions.py`
- Create: `tests/unit_tests/flux/runners/shared/test_reference_balances.py`
- Delete: `systems/flux/flux/strategies/shared/account_projection_positions.py`
- Delete: `systems/flux/flux/strategies/shared/equities_arb/reference_balances.py`

**Dependencies:** `Task 1: Lock ownership and read-model behavior in tests`

**Write Scope:** `systems/flux/flux/common/account_projection_positions.py`, `systems/flux/flux/runners/shared/reference_balances.py`, `tests/unit_tests/flux/common/test_account_projection_positions.py`, `tests/unit_tests/flux/runners/shared/test_reference_balances.py`, `systems/flux/flux/strategies/shared/account_projection_positions.py`, `systems/flux/flux/strategies/shared/equities_arb/reference_balances.py`

**Verification Commands:**
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/common/test_account_projection_positions.py tests/unit_tests/flux/runners/shared/test_reference_balances.py -q`

**Step 1: Copy the projection implementation into the common layer**

Keep behavior identical while moving the owner.

**Step 2: Move the runner-owned reference-balance provider**

Create a runner-owned home for the IBKR reference-balance provider and cache; do not leave that runtime dependency under `strategies/shared`.

**Step 3: Write or move unit tests**

The direct helper tests should now live under `tests/unit_tests/flux/common` and `tests/unit_tests/flux/runners/shared`.

**Step 4: Delete the old strategy-shared modules**

No compatibility wrapper. The old paths are removed in this PR.

**Step 5: Commit**

```bash
git add systems/flux/flux/common/account_projection_positions.py \
  systems/flux/flux/runners/shared/reference_balances.py \
  tests/unit_tests/flux/common/test_account_projection_positions.py \
  tests/unit_tests/flux/runners/shared/test_reference_balances.py \
  systems/flux/flux/strategies/shared/account_projection_positions.py \
  systems/flux/flux/strategies/shared/equities_arb/reference_balances.py
git commit -m "refactor: move projection and reference-balance helpers to proper owners"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 3: Migrate Makerv3, Makerv4, runner, and API consumers and delete the strategy-shared paths

**Files:**
- Modify: `systems/flux/flux/strategies/makerv3/strategy.py`
- Modify: `systems/flux/flux/strategies/makerv4/strategy.py`
- Modify: `systems/flux/flux/runners/shared/profile_accounts.py`
- Modify: `systems/flux/flux/runners/equities/run_node.py`
- Modify: `systems/flux/flux/strategies/shared/equities_arb/__init__.py`
- Delete: `systems/flux/flux/strategies/makerv4/reference_balances.py`
- Modify: `tests/unit_tests/flux/strategies/shared/test_equities_arb_core.py`
- Modify: `tests/unit_tests/flux/strategies/makerv3/test_inventory.py`
- Modify: `tests/unit_tests/flux/strategies/makerv4/test_inventory_contract.py`
- Modify: `tests/unit_tests/flux/strategies/makerv4/test_reference_balances.py`
- Modify: `tests/unit_tests/flux/runners/shared/test_profile_accounts.py`
- Modify: `tests/unit_tests/flux/api/test_signals_inventory_contract.py`
- Modify: `tests/unit_tests/flux/api/test_payloads.py`
- Modify: `tests/unit_tests/flux/test_architecture_boundaries.py`

**Dependencies:** `Task 2: Create common account-projection position helper and runner-owned reference-balance provider`

**Write Scope:** `systems/flux/flux/strategies/makerv3/strategy.py`, `systems/flux/flux/strategies/makerv4/strategy.py`, `systems/flux/flux/runners/shared/profile_accounts.py`, `systems/flux/flux/runners/equities/run_node.py`, `systems/flux/flux/strategies/shared/equities_arb/__init__.py`, `systems/flux/flux/strategies/makerv4/reference_balances.py`, `tests/unit_tests/flux/strategies/shared/test_equities_arb_core.py`, `tests/unit_tests/flux/strategies/makerv3/test_inventory.py`, `tests/unit_tests/flux/strategies/makerv4/test_inventory_contract.py`, `tests/unit_tests/flux/strategies/makerv4/test_reference_balances.py`, `tests/unit_tests/flux/runners/shared/test_profile_accounts.py`, `tests/unit_tests/flux/api/test_signals_inventory_contract.py`, `tests/unit_tests/flux/api/test_payloads.py`, `tests/unit_tests/flux/test_architecture_boundaries.py`

**Verification Commands:**
- `python -c "import ibapi"`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/test_architecture_boundaries.py -q -k 'reference_balances or account_projection or runner_imports'`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/strategies/shared/test_equities_arb_core.py tests/unit_tests/flux/strategies/makerv3/test_inventory.py tests/unit_tests/flux/runners/shared/test_profile_accounts.py tests/unit_tests/flux/api/test_signals_inventory_contract.py tests/unit_tests/flux/api/test_payloads.py -q`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/strategies/makerv4/test_inventory_contract.py tests/unit_tests/flux/strategies/makerv4/test_reference_balances.py -q`
- `rg -n "account_projection_positions|reference_balances" systems/flux/flux/strategies/{makerv3,makerv4,shared} systems/flux/flux/runners/{shared,equities}`

**Step 1: Update imports in Makerv3, Makerv4, runner, and API consumers**

Point both strategy families, runner consumers, and API-facing proof surfaces at the new common/runner owners. Delete the Makerv4 strategy-layer `reference_balances` surface rather than turning it into a runner re-export.

**Step 2: Re-run the strategy tests**

Confirm no payload or behavior regression. Run the Makerv3 slice in `default-unit` and the Makerv4 slice in `ibkr-unit`.

**Step 3: Verify the old paths are gone and the runner boundary is enforced**

The ripgrep check must show no remaining production dependency on the deleted strategy-shared module paths and no surviving strategy-layer `reference_balances` re-export. The architecture-boundary test must encode that strategies do not import the runner-owned provider directly.

**Step 4: Commit**

```bash
git add systems/flux/flux/strategies/makerv3/strategy.py \
  systems/flux/flux/strategies/makerv4/strategy.py \
  systems/flux/flux/runners/shared/profile_accounts.py \
  systems/flux/flux/runners/equities/run_node.py \
  systems/flux/flux/strategies/shared/equities_arb/__init__.py \
  systems/flux/flux/strategies/makerv4/reference_balances.py \
  tests/unit_tests/flux/strategies/shared/test_equities_arb_core.py \
  tests/unit_tests/flux/strategies/makerv3/test_inventory.py \
  tests/unit_tests/flux/strategies/makerv4/test_inventory_contract.py \
  tests/unit_tests/flux/strategies/makerv4/test_reference_balances.py \
  tests/unit_tests/flux/runners/shared/test_profile_accounts.py \
  tests/unit_tests/flux/api/test_signals_inventory_contract.py \
  tests/unit_tests/flux/api/test_payloads.py \
  tests/unit_tests/flux/test_architecture_boundaries.py
git commit -m "refactor: migrate consumers to common and runner ownership"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 4: Verify common/runner ownership and write rollback note

**Files:**
- Modify: `docs/plans/2026-03-31-strategy-platform-hardening-wave-pr2-shared-account-projection-ownership.md`

**Dependencies:** `Task 3: Migrate Makerv3, Makerv4, runner, and API consumers and delete the strategy-shared paths`

**Write Scope:** `docs/plans/2026-03-31-strategy-platform-hardening-wave-pr2-shared-account-projection-ownership.md`

**Verification Commands:**
- `python -c "import ibapi"`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/test_architecture_boundaries.py -q -k 'reference_balances or account_projection or runner_imports'`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/common/test_account_projection_positions.py tests/unit_tests/flux/strategies/shared/test_equities_arb_core.py tests/unit_tests/flux/runners/shared/test_profile_accounts.py tests/unit_tests/flux/api/test_signals_inventory_contract.py tests/unit_tests/flux/api/test_payloads.py tests/unit_tests/flux/strategies/makerv3/test_inventory.py -q`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/strategies/makerv4/test_inventory_contract.py tests/unit_tests/flux/strategies/makerv4/test_reference_balances.py tests/unit_tests/flux/runners/shared/test_reference_balances.py -q`
- `git diff --check`

**Step 1: Run the combined verification bundle**

This PR is done only when the common helper and both strategy consumers are green together.

**Step 2: Record rollback note**

State explicitly that rollback is safe because the helper moved but public strategy/API contracts did not.

**Step 3: Commit**

```bash
git add docs/plans/2026-03-31-strategy-platform-hardening-wave-pr2-shared-account-projection-ownership.md
git commit -m "docs: record pr2 verification and rollback"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

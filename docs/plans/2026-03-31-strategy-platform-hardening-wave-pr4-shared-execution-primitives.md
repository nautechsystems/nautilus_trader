# Strategy Platform Hardening Wave PR4 Shared Execution Primitives Implementation Plan

> **For the execution agent:** REQUIRED SUB-SKILL: Before implementing this plan, choose exactly one execution mode and use the matching skill: `superpowers:subagent-driven-development` for same-session execution or `superpowers:executing-plans` for a separate-session handoff.
> **Progress:** The Progress Tracker in this document is the execution source of truth and must be updated on every state change.

**Goal:** Move reusable managed-order helpers and strategy-facing inventory/exposure adapters out of Makerv3 ownership, migrate all current consumers, and preserve external inventory and order-safety behavior.

**Architecture:** Extract only the genuinely shared execution primitives. Managed-order collection and order-lineage helpers move to shared ownership. Strategy-facing inventory adapters move to shared ownership, but raw quantity normalization and projection truth remain in `flux.common` and do not move here because `PR2` already corrected projection ownership. This PR consumes the first-class typed market-identity contract introduced in `PR1`; it does not invent a new normalization layer. Family-local skew policy and orchestration remain local.

**Tech Stack:** Python strategy/runtime code, shared helpers, API inventory payload tests, pytest.

**Context Docs:**
- Design: `docs/plans/2026-03-31-strategy-platform-hardening-wave-design.md`
- PRD: `docs/prd/2026-03-31-strategy-platform-hardening-wave.md`
- Relevant specs/runbooks: `systems/flux/docs/makerv3.md`, `docs/plans/2026-03-31-strategy-platform-hardening-wave.md`

**Decision Summary:**
- only reusable execution primitives move
- shared-account read-model ownership is explicitly out of scope for this PR because it already moved in `PR2`
- Makerv3 and Makerv4 migrate in the same PR so no shared primitive remains dual-owned
- this PR must replace duplicated Makerv3/Makerv4 market-type heuristics in the migrated execution path with the common normalized inputs defined earlier in the wave

## Release Gates

| Environment | Required for merge? | Requirement |
| --- | --- | --- |
| `default-unit` | yes | shared, Makerv3, and API inventory/order-safety proof must pass |
| `ibkr-unit` | yes | `python -c "import ibapi"` must succeed and Makerv4 proof must pass |
| `pilot` | yes | deploy one pinned pilot release from the PR head and validate the smoke bundle below |

## Affected Pilot Surfaces

- Makerv3 and Makerv4 managed-order and cancel-safety behavior
- strategy inventory and exposure exports
- `/api/v1/signals` inventory-facing consumers

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
3. validate managed-order flow, inventory exports, and order-intent linkage
4. promote the same units together only after the smoke bundle is clean

## Pilot Smoke Bundle

1. Deploy the pilot release from the exact PR head.
2. Confirm Makerv3 and Makerv4 still publish the same inventory-facing state and order-intent linkage semantics.
3. Confirm cancel-safety, reconciliation, and managed-order flows remain stable under normal quote maintenance.
4. Confirm Makerv4 no longer imports MakerV3 managed-order or inventory helpers.

## PR-Local Rollback

- revert the whole PR
- redeploy the previous pinned pilot release
- do not leave shared execution helpers live while reverting only one consumer

## Progress Tracker

**Source of truth:** Update this table whenever task state changes. Do not rely on memory, chat history, or TodoWrite alone.

| Task | Status | Owner | Depends On | Write Scope | Lane Branch | Worktree Path | Commit / Diff | Verification | Notes / Last Update |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Overall | not_started | unassigned | none | `systems/flux/flux/strategies/shared`, `systems/flux/flux/strategies/makerv3`, `systems/flux/flux/strategies/makerv4`, `systems/flux/flux/api`, `tests/unit_tests/flux/strategies/shared`, `tests/unit_tests/flux/strategies/makerv3`, `tests/unit_tests/flux/strategies/makerv4`, `tests/unit_tests/flux/api` | `wave/pr4-execution-primitives` | `.worktrees/strategy-platform-pr4` | none | not_run | Plan created |
| Task 1: Lock shared managed-order and inventory contracts in tests | not_started | unassigned | none | `tests/unit_tests/flux/test_architecture_boundaries.py`, `tests/unit_tests/flux/common/test_market_identity.py`, `tests/unit_tests/flux/common/test_strategy_contracts.py`, `tests/unit_tests/flux/strategies/makerv3/test_managed_orders.py`, `tests/unit_tests/flux/strategies/makerv3/test_inventory.py`, `tests/unit_tests/flux/strategies/makerv4/test_inventory_contract.py`, `tests/unit_tests/flux/strategies/makerv4/test_strategy.py`, `tests/unit_tests/flux/strategies/makerv4/test_instruments.py`, `tests/unit_tests/flux/api/test_signals_inventory_contract.py`, `tests/unit_tests/flux/api/test_payload_snapshots.py` | `wave/pr4-execution-primitives` | `.worktrees/strategy-platform-pr4` | none | not_run | Plan created |
| Task 2: Extract shared managed-order helpers and migrate consumers | not_started | unassigned | Task 1: Lock shared managed-order and inventory contracts in tests | `systems/flux/flux/strategies/shared/managed_orders.py`, `systems/flux/flux/strategies/makerv3/managed_orders.py`, `systems/flux/flux/strategies/makerv4/strategy.py`, `tests/unit_tests/flux/strategies/shared/test_managed_orders.py`, `tests/unit_tests/flux/strategies/makerv3/test_managed_orders.py` | `wave/pr4-execution-primitives` | `.worktrees/strategy-platform-pr4` | none | not_run | Plan created |
| Task 3: Extract shared inventory adapters and migrate consumers | not_started | unassigned | Task 2: Extract shared managed-order helpers and migrate consumers | `systems/flux/flux/common/strategy_contracts.py`, `systems/flux/flux/strategies/shared/inventory_math.py`, `systems/flux/flux/strategies/makerv3/inventory.py`, `systems/flux/flux/strategies/makerv3/strategy.py`, `systems/flux/flux/strategies/makerv4/strategy.py`, `tests/unit_tests/flux/common/test_strategy_contracts.py`, `tests/unit_tests/flux/strategies/shared/test_inventory_math.py`, `tests/unit_tests/flux/strategies/makerv3/test_inventory.py`, `tests/unit_tests/flux/strategies/makerv4/test_inventory_contract.py`, `tests/unit_tests/flux/strategies/makerv4/test_instruments.py`, `tests/unit_tests/flux/api/test_signals_inventory_contract.py` | `wave/pr4-execution-primitives` | `.worktrees/strategy-platform-pr4` | none | not_run | Plan created |
| Task 4: Verify import cleanup and record rollback note | not_started | unassigned | Task 3: Extract shared inventory adapters and migrate consumers | `systems/flux/docs/makerv3.md`, `docs/plans/2026-03-31-strategy-platform-hardening-wave-pr4-shared-execution-primitives.md` | `wave/pr4-execution-primitives` | `.worktrees/strategy-platform-pr4` | none | not_run | Plan created |

---

### Task 1: Lock shared managed-order and inventory contracts in tests

**Files:**
- Modify: `tests/unit_tests/flux/common/test_strategy_contracts.py`
- Modify: `tests/unit_tests/flux/strategies/makerv3/test_managed_orders.py`
- Modify: `tests/unit_tests/flux/strategies/makerv3/test_inventory.py`
- Modify: `tests/unit_tests/flux/strategies/makerv4/test_inventory_contract.py`
- Modify: `tests/unit_tests/flux/strategies/makerv4/test_strategy.py`
- Modify: `tests/unit_tests/flux/strategies/makerv4/test_instruments.py`
- Modify: `tests/unit_tests/flux/api/test_signals_inventory_contract.py`
- Modify: `tests/unit_tests/flux/api/test_payload_snapshots.py`
- Modify: `tests/unit_tests/flux/test_architecture_boundaries.py`
- Modify: `tests/unit_tests/flux/common/test_market_identity.py`

**Dependencies:** `none`

**Write Scope:** `tests/unit_tests/flux/test_architecture_boundaries.py`, `tests/unit_tests/flux/common/test_market_identity.py`, `tests/unit_tests/flux/common/test_strategy_contracts.py`, `tests/unit_tests/flux/strategies/makerv3/test_managed_orders.py`, `tests/unit_tests/flux/strategies/makerv3/test_inventory.py`, `tests/unit_tests/flux/strategies/makerv4/test_inventory_contract.py`, `tests/unit_tests/flux/strategies/makerv4/test_strategy.py`, `tests/unit_tests/flux/strategies/makerv4/test_instruments.py`, `tests/unit_tests/flux/api/test_signals_inventory_contract.py`, `tests/unit_tests/flux/api/test_payload_snapshots.py`

**Verification Commands:**
- `python -c "import ibapi"`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/test_architecture_boundaries.py tests/unit_tests/flux/common/test_market_identity.py tests/unit_tests/flux/common/test_strategy_contracts.py tests/unit_tests/flux/strategies/makerv3/test_managed_orders.py tests/unit_tests/flux/strategies/makerv3/test_inventory.py tests/unit_tests/flux/strategies/makerv3/test_order_safety.py tests/unit_tests/flux/strategies/makerv3/test_reconciliation.py tests/unit_tests/flux/api/test_signals_inventory_contract.py tests/unit_tests/flux/api/test_payload_snapshots.py -q`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/strategies/makerv4/test_inventory_contract.py tests/unit_tests/flux/strategies/makerv4/test_strategy.py tests/unit_tests/flux/strategies/makerv4/test_instruments.py -q`

**Step 1: Write the failing tests**

Pin:

- managed-order collection and cancel-safety invariants
- inventory/exposure math invariants
- common market-identity normalization invariants used by the migrated helpers
- API inventory projection semantics
- representative golden fixtures for inventory and order-intent linkage payloads

**Step 2: Run tests to verify they fail**

Run the shared/Makerv3/API slice in `default-unit` and the Makerv4 slice in `ibkr-unit`.

**Step 3: Commit**

```bash
git add tests/unit_tests/flux/strategies/makerv3/test_managed_orders.py \
  tests/unit_tests/flux/strategies/makerv3/test_inventory.py \
  tests/unit_tests/flux/strategies/makerv4/test_inventory_contract.py \
  tests/unit_tests/flux/strategies/makerv4/test_strategy.py \
  tests/unit_tests/flux/strategies/makerv4/test_instruments.py \
  tests/unit_tests/flux/test_architecture_boundaries.py \
  tests/unit_tests/flux/common/test_market_identity.py \
  tests/unit_tests/flux/common/test_strategy_contracts.py \
  tests/unit_tests/flux/api/test_signals_inventory_contract.py \
  tests/unit_tests/flux/api/test_payload_snapshots.py
git commit -m "test: lock shared execution primitive contracts"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 2: Extract shared managed-order helpers and migrate consumers

**Files:**
- Create: `systems/flux/flux/strategies/shared/managed_orders.py`
- Create: `tests/unit_tests/flux/strategies/shared/test_managed_orders.py`
- Modify: `systems/flux/flux/strategies/makerv3/managed_orders.py`
- Modify: `systems/flux/flux/strategies/makerv4/strategy.py`
- Modify: `tests/unit_tests/flux/strategies/makerv3/test_managed_orders.py`

**Dependencies:** `Task 1: Lock shared managed-order and inventory contracts in tests`

**Write Scope:** `systems/flux/flux/strategies/shared/managed_orders.py`, `tests/unit_tests/flux/strategies/shared/test_managed_orders.py`, `systems/flux/flux/strategies/makerv3/managed_orders.py`, `systems/flux/flux/strategies/makerv4/strategy.py`, `tests/unit_tests/flux/strategies/makerv3/test_managed_orders.py`

**Verification Commands:**
- `python -c "import ibapi"`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/strategies/shared/test_managed_orders.py tests/unit_tests/flux/strategies/makerv3/test_managed_orders.py tests/unit_tests/flux/strategies/makerv4/test_strategy.py -q -k 'managed_order or cancel'`

**Step 1: Add direct shared tests**

Cover pure helper behavior directly under `tests/unit_tests/flux/strategies/shared`.

**Step 2: Move reusable helpers**

Extract collection, filtering, and cancel-safety primitives into `shared/managed_orders.py`.

**Step 3: Migrate Makerv3 and Makerv4**

Makerv3 and Makerv4 must both consume the shared helper in this PR.

**Step 4: Commit**

```bash
git add systems/flux/flux/strategies/shared/managed_orders.py \
  tests/unit_tests/flux/strategies/shared/test_managed_orders.py \
  systems/flux/flux/strategies/makerv3/managed_orders.py \
  systems/flux/flux/strategies/makerv4/strategy.py \
  tests/unit_tests/flux/strategies/makerv3/test_managed_orders.py
git commit -m "refactor: extract shared managed order helpers"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 3: Extract shared inventory adapters and migrate consumers

**Files:**
- Modify: `systems/flux/flux/common/strategy_contracts.py`
- Create: `systems/flux/flux/strategies/shared/inventory_math.py`
- Create: `tests/unit_tests/flux/strategies/shared/test_inventory_math.py`
- Modify: `systems/flux/flux/strategies/makerv3/inventory.py`
- Modify: `systems/flux/flux/strategies/makerv3/strategy.py`
- Modify: `systems/flux/flux/strategies/makerv4/strategy.py`
- Modify: `tests/unit_tests/flux/common/test_strategy_contracts.py`
- Modify: `tests/unit_tests/flux/strategies/makerv3/test_inventory.py`
- Modify: `tests/unit_tests/flux/strategies/makerv4/test_inventory_contract.py`
- Modify: `tests/unit_tests/flux/strategies/makerv4/test_instruments.py`
- Modify: `tests/unit_tests/flux/api/test_signals_inventory_contract.py`
- Modify: `tests/unit_tests/flux/api/test_payload_snapshots.py`
- Modify: `tests/unit_tests/flux/test_architecture_boundaries.py`

**Dependencies:** `Task 2: Extract shared managed-order helpers and migrate consumers`

**Write Scope:** `systems/flux/flux/common/strategy_contracts.py`, `systems/flux/flux/strategies/shared/inventory_math.py`, `tests/unit_tests/flux/strategies/shared/test_inventory_math.py`, `systems/flux/flux/strategies/makerv3/inventory.py`, `systems/flux/flux/strategies/makerv3/strategy.py`, `systems/flux/flux/strategies/makerv4/strategy.py`, `tests/unit_tests/flux/common/test_strategy_contracts.py`, `tests/unit_tests/flux/strategies/makerv3/test_inventory.py`, `tests/unit_tests/flux/strategies/makerv4/test_inventory_contract.py`, `tests/unit_tests/flux/strategies/makerv4/test_instruments.py`, `tests/unit_tests/flux/api/test_signals_inventory_contract.py`, `tests/unit_tests/flux/api/test_payload_snapshots.py`, `tests/unit_tests/flux/test_architecture_boundaries.py`

**Verification Commands:**
- `python -c "import ibapi"`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/test_architecture_boundaries.py tests/unit_tests/flux/common/test_market_identity.py tests/unit_tests/flux/common/test_strategy_contracts.py tests/unit_tests/flux/strategies/shared/test_inventory_math.py tests/unit_tests/flux/strategies/makerv3/test_inventory.py tests/unit_tests/flux/api/test_signals_inventory_contract.py tests/unit_tests/flux/api/test_payload_snapshots.py -q`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/strategies/makerv4/test_inventory_contract.py tests/unit_tests/flux/strategies/makerv4/test_strategy.py tests/unit_tests/flux/strategies/makerv4/test_instruments.py -q`

**Step 1: Add direct shared inventory-adapter tests**

Cover conversion and exposure math without strategy objects.

**Step 2: Move only the reusable helpers**

Do not move family-local skew or policy logic that is not clearly shared. The shared module should compose `flux.common` quantity/projection helpers rather than replacing them.

**Step 3: Remove duplicated market-type heuristics from the migrated path**

Route the migrated Makerv3 and Makerv4 execution helpers through the typed `flux.common.market_identity` surface rather than direct `-SPOT.` / `-PERP.` / `_SPOT` checks or a new hidden normalization layer.

**Step 4: Migrate Makerv3 and Makerv4**

Update both families and the API-facing tests to use the new shared helper ownership.

**Step 5: Commit**

```bash
git add systems/flux/flux/common/strategy_contracts.py \
  systems/flux/flux/strategies/shared/inventory_math.py \
  tests/unit_tests/flux/strategies/shared/test_inventory_math.py \
  systems/flux/flux/strategies/makerv3/inventory.py \
  systems/flux/flux/strategies/makerv3/strategy.py \
  systems/flux/flux/strategies/makerv4/strategy.py \
  tests/unit_tests/flux/common/test_strategy_contracts.py \
  tests/unit_tests/flux/strategies/makerv3/test_inventory.py \
  tests/unit_tests/flux/strategies/makerv4/test_inventory_contract.py \
  tests/unit_tests/flux/strategies/makerv4/test_instruments.py \
  tests/unit_tests/flux/api/test_signals_inventory_contract.py \
  tests/unit_tests/flux/api/test_payload_snapshots.py \
  tests/unit_tests/flux/test_architecture_boundaries.py
git commit -m "refactor: extract shared inventory math helpers"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 4: Verify import cleanup and record rollback note

**Files:**
- Modify: `systems/flux/docs/makerv3.md`
- Modify: `docs/plans/2026-03-31-strategy-platform-hardening-wave-pr4-shared-execution-primitives.md`

**Dependencies:** `Task 3: Extract shared inventory adapters and migrate consumers`

**Write Scope:** `systems/flux/docs/makerv3.md`, `docs/plans/2026-03-31-strategy-platform-hardening-wave-pr4-shared-execution-primitives.md`

**Verification Commands:**
- `python -c "import ibapi"`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/test_architecture_boundaries.py tests/unit_tests/flux/common/test_market_identity.py tests/unit_tests/flux/common/test_strategy_contracts.py tests/unit_tests/flux/strategies/shared/test_managed_orders.py tests/unit_tests/flux/strategies/shared/test_inventory_math.py tests/unit_tests/flux/strategies/makerv3/test_managed_orders.py tests/unit_tests/flux/strategies/makerv3/test_inventory.py tests/unit_tests/flux/strategies/makerv3/test_order_safety.py tests/unit_tests/flux/strategies/makerv3/test_reconciliation.py tests/unit_tests/flux/api/test_signals_inventory_contract.py tests/unit_tests/flux/api/test_payload_snapshots.py -q`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/strategies/makerv4/test_inventory_contract.py tests/unit_tests/flux/strategies/makerv4/test_strategy.py tests/unit_tests/flux/strategies/makerv4/test_instruments.py -q`
- `rg -n "from flux\\.strategies\\.makerv3 import inventory|from flux\\.strategies\\.makerv3 import managed_orders" systems/flux/flux/strategies/makerv4`

**Step 1: Run the combined verification bundle**

The direct shared tests, strategy tests, order-safety/reconciliation slices, and API inventory contract tests must pass together.

**Step 2: Verify Makerv4 import cleanup**

Makerv4 must no longer import the extracted helpers from Makerv3.

**Step 3: Record rollback note**

State explicitly that external payloads were preserved while internal helper ownership changed.

**Step 4: Commit**

```bash
git add systems/flux/docs/makerv3.md \
  docs/plans/2026-03-31-strategy-platform-hardening-wave-pr4-shared-execution-primitives.md
git commit -m "docs: record pr4 verification and rollback"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

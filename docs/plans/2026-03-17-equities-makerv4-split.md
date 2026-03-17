# Equities MakerV4 Split Implementation Plan

> **For the execution agent:** REQUIRED SUB-SKILL: Before implementing this plan, choose exactly one execution mode and use the matching skill: `superpowers:subagent-driven-development` for same-session execution or `superpowers:executing-plans` for a separate-session handoff.
> **Progress:** The Progress Tracker in this document is the execution source of truth and must be updated on every state change.

**Goal:** Replace the current single `makerv4` equities arb family with two explicit live strategy families, `equities_make_take` and `equities_take_take`, that can run concurrently per symbol while sharing the same equities portfolio/book and Fluxboard surface.

**Architecture:** Extract the current shared `MakerV4` hedge, fee, quote-health, and observability behavior into a reusable equities-arb shared core, then implement two thin strategy families on top of it. Keep the equities deploy topology on the current one-strategy-per-node model for now, but update the control plane so two strategy IDs can exist for the same `portfolio_asset_id` and still roll into the same shared equities portfolio and `/equities` UI.

**Tech Stack:** Python 3, Nautilus Trader strategies/runners, Flux strategy registry and API payload builders, Redis-backed params, equities deploy TOMLs and readiness checks, Fluxboard React/TypeScript signal and params surfaces, pytest, vitest.

## Progress Tracker

**Source of truth:** Update this table whenever task state changes. Do not rely on memory, chat history, or TodoWrite alone.

| Task | Status | Owner | Depends On | Write Scope | Lane Branch | Worktree Path | Commit / Diff | Verification | Notes / Last Update |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Overall | not_started | main | none | `systems/flux/flux/strategies`, `systems/flux/flux/api`, `systems/flux/flux/runners/equities`, `deploy/equities`, `ops/scripts/deploy`, `fluxboard`, `tests`, `docs/plans` | `shared` | `shared` | none | not_run | Plan created |
| Task 1: Lock Split Contract In Docs And Control-Plane Tests | not_started | unassigned | none | `docs/plans/2026-03-17-equities-makerv4-split-design.md`, `deploy/equities/README.md`, `deploy/equities/strategies/README.md`, `fluxboard/docs/equities_contract.md`, `tests/unit_tests/examples/strategies`, `tests/unit_tests/flux/api` | `shared` | `shared` | none | not_run | Plan created |
| Task 2: Extract Shared Equities-Arb Core From MakerV4 | not_started | unassigned | Task 1: Lock Split Contract In Docs And Control-Plane Tests | `systems/flux/flux/strategies/shared/equities_arb`, `systems/flux/flux/strategies/makerv4`, `tests/unit_tests/flux/strategies/shared`, `tests/unit_tests/flux/strategies/makerv4` | `shared` | `shared` | none | not_run | Plan created |
| Task 3: Add `equities_make_take` Strategy Family | not_started | unassigned | Task 2: Extract Shared Equities-Arb Core From MakerV4 | `systems/flux/flux/strategies/equities_make_take`, `systems/flux/flux/strategies/registry.py`, `systems/flux/flux/runners/equities/run_node.py`, `tests/unit_tests/flux/strategies/equities_make_take`, `tests/unit_tests/examples/strategies/test_equities_run_node.py` | `shared` | `shared` | none | not_run | Plan created |
| Task 4: Add `equities_take_take` Strategy Family | not_started | unassigned | Task 2: Extract Shared Equities-Arb Core From MakerV4 | `systems/flux/flux/strategies/equities_take_take`, `systems/flux/flux/strategies/registry.py`, `systems/flux/flux/runners/equities/run_node.py`, `tests/unit_tests/flux/strategies/equities_take_take`, `tests/unit_tests/examples/strategies/test_equities_run_node.py` | `shared` | `shared` | none | not_run | Plan created |
| Task 5: Replace MakerV4 API Metadata, Params, And Signal Payload Contracts | not_started | unassigned | Task 3: Add `equities_make_take` Strategy Family, Task 4: Add `equities_take_take` Strategy Family | `systems/flux/flux/api`, `systems/flux/flux/runners/equities/run_api.py`, `systems/flux/flux/runners/equities/run_bridge.py`, `tests/unit_tests/flux/api`, `tests/unit_tests/examples/strategies/test_equities_run_api.py`, `tests/unit_tests/examples/strategies/test_equities_run_bridge.py` | `shared` | `shared` | none | not_run | Plan created |
| Task 6: Update Fluxboard To A Shared Equities-Arb Surface | not_started | unassigned | Task 5: Replace MakerV4 API Metadata, Params, And Signal Payload Contracts | `fluxboard/components/domain/signal`, `fluxboard/config`, `fluxboard/types.ts`, `fluxboard/Params.tsx`, `fluxboard/stores.ts`, `fluxboard/tests/signal`, `fluxboard/__tests__`, `fluxboard/api.flux.test.ts` | `lanes/task-6-fluxboard` | `.worktrees/task-6-fluxboard` | none | not_run | Plan created |
| Task 7: Update Deploy, Portfolio, And Readiness Contracts For Dual Strategies Per Asset | not_started | unassigned | Task 5: Replace MakerV4 API Metadata, Params, And Signal Payload Contracts | `deploy/equities`, `deploy/equities/systemd`, `ops/scripts/deploy/equities_stack.sh`, `ops/scripts/deploy/install_equities_systemd.sh`, `systems/flux/flux/runners/equities/run_portfolio.py`, `systems/flux/flux/runners/equities/readiness.py`, `tests/unit_tests/examples/strategies`, `tests/unit_tests/flux/api/test_equities_profile_contract.py` | `lanes/task-7-deploy` | `.worktrees/task-7-deploy` | none | not_run | Plan created |
| Task 8: Remove Legacy MakerV4 Equities Contract And Run Final Verification | not_started | unassigned | Task 6: Update Fluxboard To A Shared Equities-Arb Surface, Task 7: Update Deploy, Portfolio, And Readiness Contracts For Dual Strategies Per Asset | `systems/flux/flux/strategies`, `deploy/equities`, `fluxboard`, `tests`, `docs/plans` | `shared` | `shared` | none | not_run | Plan created |

---

### Task 1: Lock Split Contract In Docs And Control-Plane Tests

**Files:**
- Modify: `docs/plans/2026-03-17-equities-makerv4-split-design.md`
- Modify: `deploy/equities/README.md`
- Modify: `fluxboard/docs/equities_contract.md`
- Modify: `deploy/equities/strategies/README.md`
- Modify: `tests/unit_tests/examples/strategies/test_equities_run_api.py`
- Modify: `tests/unit_tests/examples/strategies/test_equities_run_portfolio.py`
- Modify: `tests/unit_tests/examples/strategies/test_equities_stack_contract.py`
- Modify: `tests/unit_tests/flux/api/test_equities_profile_contract.py`

**Dependencies:** `none`

**Write Scope:** `docs/plans/2026-03-17-equities-makerv4-split-design.md`, `deploy/equities/README.md`, `fluxboard/docs/equities_contract.md`, `deploy/equities/strategies/README.md`, `tests/unit_tests/examples/strategies/test_equities_run_api.py`, `tests/unit_tests/examples/strategies/test_equities_run_portfolio.py`, `tests/unit_tests/examples/strategies/test_equities_stack_contract.py`, `tests/unit_tests/flux/api/test_equities_profile_contract.py`

**Verification Commands:**
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 uv run --group test pytest tests/unit_tests/examples/strategies/test_equities_run_api.py tests/unit_tests/examples/strategies/test_equities_run_portfolio.py tests/unit_tests/examples/strategies/test_equities_stack_contract.py -q`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 uv run --group test pytest tests/unit_tests/flux/api/test_equities_profile_contract.py -q`

**Step 1: Write the failing control-plane tests**

Add tests that require:

- two enrolled equities strategy IDs per symbol are valid
- two `strategy_contracts` rows may share the same `portfolio_asset_id`
- API metadata infers two distinct family identities from strategy IDs
- the new strategy-id suffixes resolve cleanly through the registry/run-node contract
- the split preserves the existing RTH vs outside-RTH hedge semantics and deploy knobs
- the split families do not reintroduce strategy-local inventory/risk ownership
- `take_take` is pinned as a taker-on-both-venues family, not a renamed maker-hedge mode
- the equities contract docs no longer describe one active `makerv4` row per stock

Example test shape:

```python
def test_strategy_ids_by_asset_groups_dual_equities_variants() -> None:
    assert _strategy_ids_by_asset(
        {
            "strategy_contracts": [
                _strategy_contract("aapl_tradexyz_make_take", reference_account_scope_id="ibkr.reference.main"),
                _strategy_contract("aapl_tradexyz_take_take", reference_account_scope_id="ibkr.reference.main"),
            ],
        },
        allowlist=["aapl_tradexyz_make_take", "aapl_tradexyz_take_take"],
    ) == {
        "AAPL": ("aapl_tradexyz_make_take", "aapl_tradexyz_take_take"),
    }
```

**Step 2: Run tests to verify they fail**

Run:

```bash
PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 uv run --group test pytest \
  tests/unit_tests/examples/strategies/test_equities_run_api.py \
  tests/unit_tests/examples/strategies/test_equities_run_portfolio.py \
  tests/unit_tests/examples/strategies/test_equities_stack_contract.py \
  tests/unit_tests/flux/api/test_equities_profile_contract.py -q
```

Expected: FAIL because the current repo contract still assumes one active `makerv4` family per stock.

**Step 3: Update docs to describe the new contract**

Document all of the following explicitly:

- `makerv4` is replaced, not preserved
- `make_take` and `take_take` can both run for the same symbol
- both share the same equities portfolio/book
- the exact current RTH / outside-RTH hedge semantics are preserved in wave 1
- local inventory/risk ownership knobs such as `des_qty_local`, `max_qty_local`, and `max_skew_bps_local` do not survive into the split families
- `take_take` is defined as a taker-on-both-venues strategy family
- no cross-strategy arbitration is part of this wave

**Step 4: Commit the locked contract**

Commit the docs plus the failing/updated contract tests so the rest of the plan can build on an explicit frozen contract. The passing re-run for these files is part of Task 8's final verification bundle.

```bash
git add \
  docs/plans/2026-03-17-equities-makerv4-split-design.md \
  deploy/equities/README.md \
  fluxboard/docs/equities_contract.md \
  deploy/equities/strategies/README.md \
  tests/unit_tests/examples/strategies/test_equities_run_api.py \
  tests/unit_tests/examples/strategies/test_equities_run_portfolio.py \
  tests/unit_tests/examples/strategies/test_equities_stack_contract.py \
  tests/unit_tests/flux/api/test_equities_profile_contract.py
git commit -m "test: lock equities split control-plane contract"
```

**Step 5: Update the Progress Tracker**

Mark Task 1 complete after the contract docs and failing/updated tests are committed. Record that the green verification pass for these files is deferred to Task 8.

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 2: Extract Shared Equities-Arb Core From MakerV4

**Files:**
- Create: `systems/flux/flux/strategies/shared/equities_arb/__init__.py`
- Create: `systems/flux/flux/strategies/shared/equities_arb/core.py`
- Create: `systems/flux/flux/strategies/shared/equities_arb/instruments.py`
- Create: `systems/flux/flux/strategies/shared/equities_arb/hedging.py`
- Create: `systems/flux/flux/strategies/shared/equities_arb/observability.py`
- Create: `systems/flux/flux/strategies/shared/equities_arb/reference_balances.py`
- Create: `tests/unit_tests/flux/strategies/shared/test_equities_arb_core.py`
- Modify: `systems/flux/flux/strategies/makerv4/instruments.py`
- Modify: `systems/flux/flux/strategies/makerv4/strategy.py`
- Modify: `systems/flux/flux/strategies/makerv4/managed_orders.py`
- Modify: `systems/flux/flux/strategies/makerv4/publisher.py`
- Modify: `systems/flux/flux/strategies/makerv4/reference_balances.py`
- Modify: `tests/unit_tests/flux/strategies/makerv4/test_instruments.py`
- Modify: `tests/unit_tests/flux/strategies/makerv4/test_reference_balances.py`

**Dependencies:** `Task 1: Lock Split Contract In Docs And Control-Plane Tests`

**Write Scope:** `systems/flux/flux/strategies/shared/equities_arb`, `tests/unit_tests/flux/strategies/shared/test_equities_arb_core.py`, `systems/flux/flux/strategies/makerv4/instruments.py`, `systems/flux/flux/strategies/makerv4/strategy.py`, `systems/flux/flux/strategies/makerv4/managed_orders.py`, `systems/flux/flux/strategies/makerv4/publisher.py`, `systems/flux/flux/strategies/makerv4/reference_balances.py`, `tests/unit_tests/flux/strategies/makerv4/test_instruments.py`, `tests/unit_tests/flux/strategies/makerv4/test_reference_balances.py`

**Verification Commands:**
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 uv run --group test pytest tests/unit_tests/flux/strategies/shared/test_equities_arb_core.py -q`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 uv run --group test pytest tests/unit_tests/flux/strategies/makerv4/test_strategy.py tests/unit_tests/flux/strategies/makerv4/test_publisher_contract.py tests/unit_tests/flux/strategies/makerv4/test_instruments.py tests/unit_tests/flux/strategies/makerv4/test_reference_balances.py -q`

**Step 1: Write failing shared-core tests**

Add tests that pin reusable behavior for:

- shared fee assumptions payload
- shared hedge policy / pending hedge payload shape
- shared backlog payload shape
- shared quote snapshot assembly for equities arb legs
- shared session-aware hedge policy behavior used by both families
- runner-facing instrument/reference-balance helpers that must stop being `makerv4`-owned

**Step 2: Run the new shared-core tests and confirm failure**

Run:

```bash
PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 uv run --group test pytest \
  tests/unit_tests/flux/strategies/shared/test_equities_arb_core.py -q
```

Expected: FAIL because the shared equities-arb module does not exist.

**Step 3: Extract the minimal shared core**

Move the reusable pieces out of `makerv4`:

- hedge/backlog intent and payload assembly
- fee assumptions payload helpers
- shared leg/quote snapshot assembly
- session-aware hedge policy helpers
- instrument mapping and reference-balance helpers used by the equities runner

Do not rename the live family yet. This task is only about isolating the reusable seam.

**Step 4: Re-run the shared-core and MakerV4 regression slice**

Expected: PASS with no behavior drift.

**Step 5: Commit**

```bash
git add \
  systems/flux/flux/strategies/shared/equities_arb \
  tests/unit_tests/flux/strategies/shared/test_equities_arb_core.py \
  systems/flux/flux/strategies/makerv4/instruments.py \
  systems/flux/flux/strategies/makerv4/strategy.py \
  systems/flux/flux/strategies/makerv4/managed_orders.py \
  systems/flux/flux/strategies/makerv4/publisher.py \
  systems/flux/flux/strategies/makerv4/reference_balances.py \
  tests/unit_tests/flux/strategies/makerv4/test_instruments.py \
  tests/unit_tests/flux/strategies/makerv4/test_reference_balances.py
git commit -m "refactor: extract shared equities arb core"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 3: Add `equities_make_take` Strategy Family

**Files:**
- Create: `systems/flux/flux/strategies/equities_make_take/__init__.py`
- Create: `systems/flux/flux/strategies/equities_make_take/constants.py`
- Create: `systems/flux/flux/strategies/equities_make_take/runtime_params.py`
- Create: `systems/flux/flux/strategies/equities_make_take/strategy.py`
- Create: `tests/unit_tests/flux/strategies/equities_make_take/test_runtime_params.py`
- Create: `tests/unit_tests/flux/strategies/equities_make_take/test_strategy.py`
- Modify: `systems/flux/flux/strategies/registry.py`
- Modify: `systems/flux/flux/strategies/__init__.py`
- Modify: `systems/flux/flux/runners/equities/run_node.py`
- Modify: `tests/unit_tests/examples/strategies/test_equities_run_node.py`

**Dependencies:** `Task 2: Extract Shared Equities-Arb Core From MakerV4`

**Write Scope:** `systems/flux/flux/strategies/equities_make_take`, `systems/flux/flux/strategies/registry.py`, `systems/flux/flux/strategies/__init__.py`, `systems/flux/flux/runners/equities/run_node.py`, `tests/unit_tests/flux/strategies/equities_make_take/test_runtime_params.py`, `tests/unit_tests/flux/strategies/equities_make_take/test_strategy.py`, `tests/unit_tests/examples/strategies/test_equities_run_node.py`

**Verification Commands:**
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 uv run --group test pytest tests/unit_tests/flux/strategies/equities_make_take/test_runtime_params.py tests/unit_tests/flux/strategies/equities_make_take/test_strategy.py -q`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 uv run --group test pytest tests/unit_tests/examples/strategies/test_equities_run_node.py -q`

**Step 1: Write the failing `make_take` family tests**

Pin:

- registry identity
- suffix-based strategy-id resolution for `<symbol>_tradexyz_make_take`
- runtime param surface without `take_take`-only knobs or local inventory/risk ownership knobs
- exact preservation of the current RTH / outside-RTH hedge semantics
- maker quote lifecycle uses the shared equities-arb core
- shared portfolio/book risk is still the only asset-level risk source

**Step 2: Run tests to verify failure**

Expected: FAIL because `equities_make_take` does not exist.

**Step 3: Implement the thin family wrapper**

Create `EquitiesMakeTakeStrategy` and config/runtime modules that:

- consume the shared core
- expose only the common plus make-take-specific runtime params
- keep the existing session-aware hedge contract unchanged
- omit `des_qty_local`, `max_qty_local`, `max_skew_bps_local`, and equivalent local-inventory controls
- preserve current maker-side quote behavior

**Step 4: Re-run the focused `make_take` tests**

Expected: PASS.

**Step 5: Commit**

```bash
git add \
  systems/flux/flux/strategies/equities_make_take \
  systems/flux/flux/strategies/registry.py \
  systems/flux/flux/strategies/__init__.py \
  systems/flux/flux/runners/equities/run_node.py \
  tests/unit_tests/flux/strategies/equities_make_take/test_runtime_params.py \
  tests/unit_tests/flux/strategies/equities_make_take/test_strategy.py \
  tests/unit_tests/examples/strategies/test_equities_run_node.py
git commit -m "feat: add equities make-take strategy family"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 4: Add `equities_take_take` Strategy Family

**Files:**
- Create: `systems/flux/flux/strategies/equities_take_take/__init__.py`
- Create: `systems/flux/flux/strategies/equities_take_take/constants.py`
- Create: `systems/flux/flux/strategies/equities_take_take/runtime_params.py`
- Create: `systems/flux/flux/strategies/equities_take_take/strategy.py`
- Create: `tests/unit_tests/flux/strategies/equities_take_take/test_runtime_params.py`
- Create: `tests/unit_tests/flux/strategies/equities_take_take/test_strategy.py`
- Modify: `systems/flux/flux/strategies/registry.py`
- Modify: `systems/flux/flux/strategies/__init__.py`
- Modify: `systems/flux/flux/runners/equities/run_node.py`
- Modify: `tests/unit_tests/examples/strategies/test_equities_run_node.py`

**Dependencies:** `Task 2: Extract Shared Equities-Arb Core From MakerV4`

**Write Scope:** `systems/flux/flux/strategies/equities_take_take`, `systems/flux/flux/strategies/registry.py`, `systems/flux/flux/strategies/__init__.py`, `systems/flux/flux/runners/equities/run_node.py`, `tests/unit_tests/flux/strategies/equities_take_take/test_runtime_params.py`, `tests/unit_tests/flux/strategies/equities_take_take/test_strategy.py`, `tests/unit_tests/examples/strategies/test_equities_run_node.py`

**Verification Commands:**
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 uv run --group test pytest tests/unit_tests/flux/strategies/equities_take_take/test_runtime_params.py tests/unit_tests/flux/strategies/equities_take_take/test_strategy.py -q`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 uv run --group test pytest tests/unit_tests/examples/strategies/test_equities_run_node.py -q`

**Step 1: Write the failing `take_take` family tests**

Pin:

- registry identity
- suffix-based strategy-id resolution for `<symbol>_tradexyz_take_take`
- runtime params keep only take-take-relevant knobs and omit local inventory/risk ownership knobs
- `take_take` signal generation and execution are family-owned, not hidden behind `execution_mode`
- `take_take` is pinned as a taker-on-both-venues strategy rather than a maker quote loop
- exact preservation of the current RTH / outside-RTH hedge semantics
- hedge backlog and shared portfolio-risk reads still work through the shared core

**Step 2: Run tests to verify failure**

Expected: FAIL because `equities_take_take` does not exist.

**Step 3: Implement the thin family wrapper**

Create `EquitiesTakeTakeStrategy` and config/runtime modules that:

- consume the shared core
- expose take-take-specific params such as threshold and cooldown knobs
- keep the current aggressive outside-band behavior and shared hedge path
- implement a taker-on-both-venues execution model instead of reusing the maker quote loop
- omit `des_qty_local`, `max_qty_local`, `max_skew_bps_local`, and equivalent local-inventory controls

**Step 4: Re-run the focused `take_take` tests**

Expected: PASS.

**Step 5: Commit**

```bash
git add \
  systems/flux/flux/strategies/equities_take_take \
  systems/flux/flux/strategies/registry.py \
  systems/flux/flux/strategies/__init__.py \
  systems/flux/flux/runners/equities/run_node.py \
  tests/unit_tests/flux/strategies/equities_take_take/test_runtime_params.py \
  tests/unit_tests/flux/strategies/equities_take_take/test_strategy.py \
  tests/unit_tests/examples/strategies/test_equities_run_node.py
git commit -m "feat: add equities take-take strategy family"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 5: Replace MakerV4 API Metadata, Params, And Signal Payload Contracts

**Files:**
- Modify: `systems/flux/flux/api/app.py`
- Modify: `systems/flux/flux/runners/equities/run_api.py`
- Modify: `systems/flux/flux/runners/equities/run_bridge.py`
- Modify: `systems/flux/flux/api/_payloads_signals.py`
- Modify: `fluxboard/types.ts`
- Modify: `tests/unit_tests/examples/strategies/test_equities_run_api.py`
- Modify: `tests/unit_tests/examples/strategies/test_equities_run_bridge.py`
- Modify: `tests/unit_tests/flux/api/test_equities_profile_contract.py`
- Modify: `tests/unit_tests/flux/api/test_payloads.py`

**Dependencies:** `Task 3: Add \`equities_make_take\` Strategy Family`, `Task 4: Add \`equities_take_take\` Strategy Family`

**Write Scope:** `systems/flux/flux/api/app.py`, `systems/flux/flux/runners/equities/run_api.py`, `systems/flux/flux/runners/equities/run_bridge.py`, `systems/flux/flux/api/_payloads_signals.py`, `fluxboard/types.ts`, `tests/unit_tests/examples/strategies/test_equities_run_api.py`, `tests/unit_tests/examples/strategies/test_equities_run_bridge.py`, `tests/unit_tests/flux/api/test_equities_profile_contract.py`, `tests/unit_tests/flux/api/test_payloads.py`

**Verification Commands:**
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 uv run --group test pytest tests/unit_tests/examples/strategies/test_equities_run_api.py tests/unit_tests/examples/strategies/test_equities_run_bridge.py tests/unit_tests/flux/api/test_equities_profile_contract.py tests/unit_tests/flux/api/test_payloads.py -q`

**Step 1: Write the failing API and payload tests**

Require:

- strategy metadata recognizes `aapl_tradexyz_make_take` and `aapl_tradexyz_take_take`
- the params API can serve genuinely different `params_schema`, `params_defaults`, and `param_set` contracts for the two families
- signal payloads expose one shared equities-arb operator contract
- fee assumptions and pricing fields remain visible for both variants
- family-specific rows still share common leg/quote-health semantics
- bridge allowlist and explicit strategy-id resolution accept both variants cleanly

**Step 2: Run tests to verify failure**

Expected: FAIL because the API currently recognizes `maker_v4`-specific semantics.

**Step 3: Update metadata and payload builders**

Implement:

- family-aware params schema/default selection in `flux.api.app`
- new strategy-id-to-spec resolution
- family-aware metadata emission
- shared equities-arb payload shape replacing the hard-coded MakerV4 contract

**Step 4: Re-run the focused API suite**

Expected: PASS.

**Step 5: Commit**

```bash
git add \
  systems/flux/flux/api/app.py \
  systems/flux/flux/runners/equities/run_api.py \
  systems/flux/flux/runners/equities/run_bridge.py \
  systems/flux/flux/api/_payloads_signals.py \
  fluxboard/types.ts \
  tests/unit_tests/examples/strategies/test_equities_run_api.py \
  tests/unit_tests/examples/strategies/test_equities_run_bridge.py \
  tests/unit_tests/flux/api/test_equities_profile_contract.py \
  tests/unit_tests/flux/api/test_payloads.py
git commit -m "feat: add dual equities arb api contract"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 6: Update Fluxboard To A Shared Equities-Arb Surface

**Files:**
- Create: `fluxboard/components/domain/signal/EquitiesArbSignalTable.tsx`
- Modify: `fluxboard/components/domain/signal/SignalTable.tsx`
- Modify: `fluxboard/Params.tsx`
- Modify: `fluxboard/config/paramsProfiles.ts`
- Modify: `fluxboard/components/panels/ParamsPanel.tsx`
- Modify: `fluxboard/stores.ts`
- Modify: `fluxboard/api.flux.test.ts`
- Modify: `fluxboard/tests/signal/MakerV4SignalTable.test.tsx`
- Create: `fluxboard/tests/signal/EquitiesArbSignalTable.test.tsx`
- Modify: `fluxboard/tests/signal/SignalFamilyFilter.test.tsx`
- Modify: `fluxboard/__tests__/components/ParamsProfileColumns.test.tsx`
- Modify: `fluxboard/__tests__/Params.short-headers.test.tsx`
- Modify: `fluxboard/__tests__/config/paramsProfiles.test.ts`

**Dependencies:** `Task 5: Replace MakerV4 API Metadata, Params, And Signal Payload Contracts`

**Write Scope:** `fluxboard/components/domain/signal/EquitiesArbSignalTable.tsx`, `fluxboard/components/domain/signal/SignalTable.tsx`, `fluxboard/Params.tsx`, `fluxboard/config/paramsProfiles.ts`, `fluxboard/components/panels/ParamsPanel.tsx`, `fluxboard/stores.ts`, `fluxboard/api.flux.test.ts`, `fluxboard/tests/signal/EquitiesArbSignalTable.test.tsx`, `fluxboard/tests/signal/MakerV4SignalTable.test.tsx`, `fluxboard/tests/signal/SignalFamilyFilter.test.tsx`, `fluxboard/__tests__/components/ParamsProfileColumns.test.tsx`, `fluxboard/__tests__/Params.short-headers.test.tsx`, `fluxboard/__tests__/config/paramsProfiles.test.ts`

**Verification Commands:**
- `pnpm --dir fluxboard exec vitest run tests/signal/EquitiesArbSignalTable.test.tsx tests/signal/SignalFamilyFilter.test.tsx api.flux.test.ts`
- `pnpm --dir fluxboard exec vitest run __tests__/components/ParamsProfileColumns.test.tsx __tests__/Params.short-headers.test.tsx __tests__/config/paramsProfiles.test.ts`

**Step 1: Write the failing Fluxboard tests**

Require:

- the equities signal route renders one shared table for both variants
- rows include a visible variant label
- the family filter and params profile logic no longer assume `maker_v4`
- rows sort/group by symbol then variant
- the params route shows common controls first, then session/shared-risk controls, then family-specific controls
- local inventory/risk controls such as `des_qty_local`, `max_qty_local`, and `max_skew_bps_local` are absent from the split equities profiles
- API client metadata normalization still recognizes the split families and param sets
- persisted UI state and params selection logic handle the split families cleanly

**Step 2: Run tests to verify failure**

Expected: FAIL because the UI still routes equities through `MakerV4SignalTable` and `maker_v4` params assumptions.

**Step 3: Implement the shared equities-arb surface**

Build a shared table that:

- reuses the existing leg and operator affordances where possible
- adds explicit variant labeling
- keeps fee and hedge-policy observability visible for both variants
- updates Params profile routing, canonical ordering, and persisted store state for the shared `/equities/params` workflow

**Step 4: Re-run the focused Fluxboard suite**

Expected: PASS.

**Step 5: Commit**

```bash
git add \
  fluxboard/components/domain/signal/EquitiesArbSignalTable.tsx \
  fluxboard/components/domain/signal/SignalTable.tsx \
  fluxboard/Params.tsx \
  fluxboard/config/paramsProfiles.ts \
  fluxboard/components/panels/ParamsPanel.tsx \
  fluxboard/stores.ts \
  fluxboard/api.flux.test.ts \
  fluxboard/tests/signal/EquitiesArbSignalTable.test.tsx \
  fluxboard/tests/signal/MakerV4SignalTable.test.tsx \
  fluxboard/tests/signal/SignalFamilyFilter.test.tsx \
  fluxboard/__tests__/components/ParamsProfileColumns.test.tsx \
  fluxboard/__tests__/Params.short-headers.test.tsx \
  fluxboard/__tests__/config/paramsProfiles.test.ts
git commit -m "feat: add shared equities arb fluxboard surface"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 7: Update Deploy, Portfolio, And Readiness Contracts For Dual Strategies Per Asset

**Files:**
- Modify: `deploy/equities/equities.live.toml`
- Modify: `deploy/equities/README.md`
- Modify: `deploy/equities/systemd/flux-equities.target`
- Modify: `deploy/equities/systemd/flux-pulse.sudoers`
- Modify: `deploy/equities/strategies/equities.strategy.template.toml`
- Modify: `deploy/equities/strategies/*.toml`
- Modify: `deploy/equities/strategies/README.md`
- Modify: `ops/scripts/deploy/equities_stack.sh`
- Modify: `ops/scripts/deploy/install_equities_systemd.sh`
- Modify: `systems/flux/flux/runners/equities/run_portfolio.py`
- Modify: `systems/flux/flux/runners/equities/readiness.py`
- Modify: `tests/unit_tests/examples/strategies/test_equities_run_portfolio.py`
- Modify: `tests/unit_tests/examples/strategies/test_equities_stack_contract.py`
- Modify: `tests/unit_tests/examples/strategies/test_equities_readiness.py`

**Dependencies:** `Task 5: Replace MakerV4 API Metadata, Params, And Signal Payload Contracts`

**Write Scope:** `deploy/equities/equities.live.toml`, `deploy/equities/README.md`, `deploy/equities/systemd/flux-equities.target`, `deploy/equities/systemd/flux-pulse.sudoers`, `deploy/equities/strategies/equities.strategy.template.toml`, `deploy/equities/strategies/*.toml`, `deploy/equities/strategies/README.md`, `ops/scripts/deploy/equities_stack.sh`, `ops/scripts/deploy/install_equities_systemd.sh`, `systems/flux/flux/runners/equities/run_portfolio.py`, `systems/flux/flux/runners/equities/readiness.py`, `tests/unit_tests/examples/strategies/test_equities_run_portfolio.py`, `tests/unit_tests/examples/strategies/test_equities_stack_contract.py`, `tests/unit_tests/examples/strategies/test_equities_readiness.py`

**Verification Commands:**
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 uv run --group test pytest tests/unit_tests/examples/strategies/test_equities_run_portfolio.py tests/unit_tests/examples/strategies/test_equities_stack_contract.py tests/unit_tests/examples/strategies/test_equities_readiness.py -q`

**Step 1: Write the failing deploy/readiness tests**

Require:

- two active strategy IDs per symbol are allowed in `equities.live.toml`
- `strategy_contracts` may repeat `portfolio_asset_id`
- readiness expects both enrolled strategies while still checking shared asset-level portfolio health
- the template and README use the new naming pattern
- actual per-variant strategy TOMLs exist under the current one-strategy-per-node deploy model
- the split keeps the existing session contract, including `use_regular_trading_hours = false` and `outside_rth_hedge_enabled`, for both families
- templates and deploy profiles do not surface local inventory/risk ownership knobs
- stack/install scripts discover, install, and launch both variants per symbol without hand-edited service drift

**Step 2: Run tests to verify failure**

Expected: FAIL because the current stack contract still assumes one active strategy per asset.

**Step 3: Update deploy and portfolio/readiness logic**

Implement:

- dual-strategy allowlists
- dual `strategy_contracts` rows per asset
- actual per-variant strategy files replacing the live `makerv4` TOMLs
- shared-asset grouping in portfolio aggregation
- readiness summaries and contract docs that understand the split
- systemd/host-install surfaces that reflect the new per-variant deploy artifacts

**Step 4: Re-run the focused deploy/readiness suite**

Expected: PASS.

**Step 5: Commit**

```bash
git add \
  deploy/equities/equities.live.toml \
  deploy/equities/README.md \
  deploy/equities/systemd/flux-equities.target \
  deploy/equities/systemd/flux-pulse.sudoers \
  deploy/equities/strategies/equities.strategy.template.toml \
  deploy/equities/strategies/*.toml \
  deploy/equities/strategies/README.md \
  ops/scripts/deploy/equities_stack.sh \
  ops/scripts/deploy/install_equities_systemd.sh \
  systems/flux/flux/runners/equities/run_portfolio.py \
  systems/flux/flux/runners/equities/readiness.py \
  tests/unit_tests/examples/strategies/test_equities_run_portfolio.py \
  tests/unit_tests/examples/strategies/test_equities_stack_contract.py \
  tests/unit_tests/examples/strategies/test_equities_readiness.py
git commit -m "feat: add dual-strategy equities deploy contract"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 8: Remove Legacy MakerV4 Equities Contract And Run Final Verification

**Files:**
- Modify: `systems/flux/flux/strategies/makerv4/__init__.py`
- Modify: `systems/flux/flux/strategies/makerv4/constants.py`
- Modify: `systems/flux/flux/strategies/__init__.py`
- Modify: `systems/flux/flux/strategies/registry.py`
- Modify: `deploy/equities/equities.live.toml`
- Modify: `deploy/equities/strategies/*.toml`
- Modify: `fluxboard/components/domain/signal/MakerV4SignalTable.tsx`
- Modify: `fluxboard/docs/equities_contract.md`
- Modify: `docs/plans/2026-03-17-equities-makerv4-split-design.md`
- Modify: `docs/plans/2026-03-17-equities-makerv4-split.md`
- Modify: `tests/unit_tests/flux/strategies/makerv4/test_observability_and_exports.py`

**Dependencies:** `Task 6: Update Fluxboard To A Shared Equities-Arb Surface`, `Task 7: Update Deploy, Portfolio, And Readiness Contracts For Dual Strategies Per Asset`

**Write Scope:** `systems/flux/flux/strategies`, `deploy/equities`, `fluxboard/components/domain/signal/MakerV4SignalTable.tsx`, `fluxboard/docs/equities_contract.md`, `docs/plans/2026-03-17-equities-makerv4-split-design.md`, `docs/plans/2026-03-17-equities-makerv4-split.md`, `tests`

**Verification Commands:**
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 uv run --group test pytest tests/unit_tests/flux/strategies/shared/test_equities_arb_core.py tests/unit_tests/flux/strategies/equities_make_take/test_runtime_params.py tests/unit_tests/flux/strategies/equities_make_take/test_strategy.py tests/unit_tests/flux/strategies/equities_take_take/test_runtime_params.py tests/unit_tests/flux/strategies/equities_take_take/test_strategy.py tests/unit_tests/flux/strategies/makerv4/test_observability_and_exports.py tests/unit_tests/examples/strategies/test_equities_run_api.py tests/unit_tests/examples/strategies/test_equities_run_bridge.py tests/unit_tests/examples/strategies/test_equities_run_node.py tests/unit_tests/examples/strategies/test_equities_run_portfolio.py tests/unit_tests/examples/strategies/test_equities_stack_contract.py tests/unit_tests/examples/strategies/test_equities_readiness.py tests/unit_tests/flux/api/test_equities_profile_contract.py tests/unit_tests/flux/api/test_payloads.py -q`
- `pnpm --dir fluxboard exec vitest run tests/signal/EquitiesArbSignalTable.test.tsx tests/signal/SignalFamilyFilter.test.tsx __tests__/components/ParamsProfileColumns.test.tsx __tests__/Params.short-headers.test.tsx __tests__/config/paramsProfiles.test.ts api.flux.test.ts`
- `git diff --check`

**Step 1: Write the final failing cleanup assertions**

Add or update tests so they require:

- no active equities deploy/config/docs refer to `makerv4` as the live contract
- the shared UI and API only describe the split families
- the final active strategy IDs use the new suffixes
- the params surfaces no longer expose local inventory/risk ownership knobs
- the preserved RTH / outside-RTH hedge semantics still match the prior live contract

**Step 2: Run the final suite and confirm remaining failures are only the cleanup gap**

Expected: FAIL until the last MakerV4-specific references are removed.

**Step 3: Remove the remaining live MakerV4 equities contract**

Clean up:

- active deploy references
- live docs
- stale family-specific assumptions that are no longer exercised by equities
- registry/export assumptions that still treat `makerv4` as the active equities family
- the dedicated MakerV4 Fluxboard surface if it is no longer used by any active route

Keep any generic shared code still used by the new families.

**Step 4: Re-run the final verification bundle**

Expected: PASS.

**Step 5: Commit**

```bash
git add \
  systems/flux/flux/strategies \
  deploy/equities \
  fluxboard/components/domain/signal/MakerV4SignalTable.tsx \
  fluxboard/docs/equities_contract.md \
  docs/plans/2026-03-17-equities-makerv4-split-design.md \
  docs/plans/2026-03-17-equities-makerv4-split.md \
  tests
git commit -m "feat: replace equities makerv4 with split arb families"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

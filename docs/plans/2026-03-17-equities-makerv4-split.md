# Equities MakerV4 Split Implementation Plan

> **For the execution agent:** REQUIRED SUB-SKILL: Before implementing this plan, choose exactly one execution mode and use the matching skill: `superpowers:subagent-driven-development` for same-session execution or `superpowers:executing-plans` for a separate-session handoff.
> **Progress:** The Progress Tracker in this document is the execution source of truth and must be updated on every state change.

**Goal:** Replace the current single `makerv4` equities arb family with two explicit live strategy families, `equities_maker` and `equities_taker`, that can run concurrently per symbol while sharing the same equities portfolio/book and Fluxboard surface.

**Architecture:** Extract the current shared `MakerV4` hedge, fee, quote-health, and observability behavior into a reusable equities-arb shared core, then implement two family-specific strategies on top of it. `equities_maker` can stay relatively thin after extraction; `equities_taker` is likely a more substantial extraction because its current behavior is interleaved into the `makerv4` state machine. Keep the equities deploy topology on the current one-strategy-per-node model for now, but update the control plane so two strategy IDs can exist for the same `portfolio_asset_id` and still roll into the same shared equities portfolio and `/equities` UI.

**Tech Stack:** Python 3, Nautilus Trader strategies/runners, Flux strategy registry and API payload builders, Redis-backed params, equities deploy TOMLs and readiness checks, Fluxboard React/TypeScript signal and params surfaces, pytest, vitest.

## Progress Tracker

**Source of truth:** Update this table whenever task state changes. Do not rely on memory, chat history, or TodoWrite alone.

| Task | Status | Owner | Depends On | Write Scope | Lane Branch | Worktree Path | Commit / Diff | Verification | Notes / Last Update |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Overall | in_progress | main | none | `systems/flux/flux/strategies`, `systems/flux/flux/api`, `systems/flux/flux/runners/equities`, `deploy/equities`, `ops/scripts/deploy`, `fluxboard`, `tests`, `docs/plans` | `shared` | `shared` | `29dc2906fd`, `b2b5a9ae67`, `e8e34737ea`, `e13854360c`, `6a6d2dd9e5`, `3943dd689a`, `1e04ff1f33`, `350d0d4fe5`, `e0a7512ddf`, `7ef3c94c30`, `765554b919` + Task 6 tracker updates | Task 0, Task 2, Task 3, Task 4, and Task 5 verification bundles passing on the shared branch; Task 6 exact Fluxboard verification also passes on the shared branch after integration (`5` files / `65` tests and `3` files / `27` tests); Task 7 implementation commit is now green in the lane on both the plan suite (`68` tests) and the extra `run_api` control-plane regression file (`21` tests) | Task 0 naming alignment and Tasks 2-5 completed. Task 6 is now completed on the shared branch: the approved Fluxboard lane series was integrated from the Task 5 base as shared commits `e8e34737ea` through `765554b919`, a fresh quality re-review on the full Task 6 diff found no findings, and the exact Task 6 verification bundles were rerun successfully in the shared worktree after integration. Task 7 started on 2026-03-19 UTC with dedicated lane branch `lanes/task-7-deploy-readiness` in `.worktrees/task-7-deploy-readiness`; a fresh Task 7 worker stalled before the red phase, so the controller reclaimed local TDD execution in the same lane worktree, finished the cold `uv sync --all-groups --all-extras`, confirmed the red deploy/API gaps, and committed the green slice as `9b5919fd8f` before spec review |
| Task 0: Align Approved `equities_maker` / `equities_taker` Naming | completed | main | Task 1: Lock Split Contract In Docs And Execution Matrix | `docs/plans/2026-03-17-equities-makerv4-split.md`, `docs/plans/2026-03-17-equities-makerv4-split-design.md`, `GitHub PR #64 title/body` | `shared` | `shared` | `9c27e7c708` | `git diff --check` pass | 2026-03-17 UTC docs/PR naming aligned in `9c27e7c708`; controller spec review and quality review passed after subagent timeouts |
| Task 1: Lock Split Contract In Docs And Execution Matrix | completed | main | none | `docs/plans/2026-03-17-equities-makerv4-split-design.md`, `deploy/equities/README.md`, `deploy/equities/strategies/README.md`, `fluxboard/docs/equities_contract.md`, `docs/plans/2026-03-17-equities-makerv4-split.md` | `shared` | `shared` | `929dad49a0` | `git diff --check` pass | Completed on planning branch before implementation bootstrap |
| Task 2: Extract Shared Equities-Arb Core From MakerV4 | completed | main | Task 1: Lock Split Contract In Docs And Execution Matrix | `systems/flux/flux/strategies/shared/equities_arb`, `systems/flux/flux/strategies/shared/ibkr_order_policy.py`, `systems/flux/flux/strategies/makerv4`, `systems/flux/flux/runners/equities/run_node.py`, `systems/flux/flux/runners/shared/profile_accounts.py`, `tests/unit_tests/flux/strategies/shared`, `tests/unit_tests/flux/strategies/shared/test_ibkr_order_policy.py`, `tests/unit_tests/flux/strategies/makerv4`, `tests/unit_tests/examples/strategies` | `shared` | `shared` | `1a80ba3c6d`, `5bf8cdb7c2`, `16b0e1f956` | `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 uv run --group test pytest tests/unit_tests/flux/strategies/shared/test_equities_arb_core.py -q` pass; `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 uv run --group test pytest tests/unit_tests/flux/strategies/makerv4/test_strategy.py tests/unit_tests/flux/strategies/makerv4/test_publisher_contract.py tests/unit_tests/flux/strategies/makerv4/test_instruments.py tests/unit_tests/flux/strategies/makerv4/test_pricing.py tests/unit_tests/flux/strategies/makerv4/test_reference_balances.py tests/unit_tests/examples/strategies/test_equities_run_node.py tests/unit_tests/examples/strategies/test_equities_run_portfolio.py -q` pass | 2026-03-17 UTC implementation landed in `1a80ba3c6d`, scope cleanup in `5bf8cdb7c2`, and quality-fix hardening in `16b0e1f956`; spec review passed with no findings, quality review findings were fixed, and quality re-review passed with no findings |
| Task 3: Add `equities_maker` Strategy Family | completed | main | Task 2: Extract Shared Equities-Arb Core From MakerV4 | `systems/flux/flux/strategies/equities_maker`, `systems/flux/flux/strategies/registry.py`, `systems/flux/flux/strategies/__init__.py`, `systems/flux/flux/runners/equities/run_node.py`, `tests/unit_tests/flux/strategies/equities_maker`, `tests/unit_tests/examples/strategies/test_equities_run_node.py` | `shared` | `shared` | `533e6b72a0` | `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 uv run --group test pytest tests/unit_tests/flux/strategies/equities_maker/test_runtime_params.py tests/unit_tests/flux/strategies/equities_maker/test_strategy.py -q` pass; `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 uv run --group test pytest tests/unit_tests/examples/strategies/test_equities_run_node.py -q` pass | 2026-03-18 UTC red step confirmed on config omission and run_node local-field omission; Task 3 committed in `533e6b72a0`; spec review passed with no findings and quality review passed with no findings |
| Task 4: Add `equities_taker` Strategy Family | completed | main | Task 2: Extract Shared Equities-Arb Core From MakerV4 | `systems/flux/flux/strategies/equities_taker`, `systems/flux/flux/strategies/registry.py`, `systems/flux/flux/strategies/__init__.py`, `systems/flux/flux/runners/equities/run_node.py`, `tests/unit_tests/flux/strategies/equities_taker`, `tests/unit_tests/examples/strategies/test_equities_run_node.py` | `shared` | `shared` | `500ad64669`, `d54aabd1d2`, `9f34bc6e47`, `ac749253ed` | `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 uv run --group test pytest tests/unit_tests/flux/strategies/equities_taker/test_runtime_params.py tests/unit_tests/flux/strategies/equities_taker/test_strategy.py -q` pass; `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 uv run --group test pytest tests/unit_tests/examples/strategies/test_equities_run_node.py -q` pass | 2026-03-18 UTC fresh implementer lane returned without reaching red step; controller reclaimed Task 4 for local TDD execution, confirmed the red step on missing `equities_taker` package/root exports and runner import path, committed the green slice in `500ad64669`, failed spec review on family-owned taker dispatch and `ibkr_hedge_route` passthrough, reproduced both gaps with failing tests, fixed them in `d54aabd1d2`, passed spec re-review with no findings, failed quality review on runtime-param seeding from config and missing pricing-debug observability updates, fixed those in `9f34bc6e47`, fixed the remaining `order_qty` sizing-default gap in `ac749253ed`, and the quality re-review passed with no findings |
| Task 5: Replace MakerV4 API Metadata, Strategy-Scoped Params, Signal Payload, And Readiness Contracts | completed | main | Task 3: Add `equities_maker` Strategy Family, Task 4: Add `equities_taker` Strategy Family | `systems/flux/flux/api`, `systems/flux/flux/runners/equities/run_api.py`, `systems/flux/flux/runners/equities/run_bridge.py`, `systems/flux/flux/runners/equities/readiness.py`, `tests/unit_tests/flux/api`, `tests/unit_tests/examples/strategies/test_equities_run_api.py`, `tests/unit_tests/examples/strategies/test_equities_run_bridge.py`, `tests/unit_tests/examples/strategies/test_equities_readiness.py` | `shared` | `shared` | `4300856183` | `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 uv run --group test pytest tests/unit_tests/flux/api/test_app.py tests/unit_tests/examples/strategies/test_equities_run_api.py tests/unit_tests/examples/strategies/test_equities_run_bridge.py tests/unit_tests/examples/strategies/test_equities_readiness.py tests/unit_tests/flux/api/test_equities_profile_contract.py tests/unit_tests/flux/api/test_payloads.py -q` pass | 2026-03-19 UTC Task 5 red baseline was reproduced first, then the green slice landed in `4300856183` with per-family params contracts, shared `equities_arb` signal payloads, readiness parsing on the shared contract, and `run_api.main()` metadata binding through root `strategy_contracts`; focused verification passed locally, spec review found no findings, and quality review found no findings |
| Task 6: Update Fluxboard To A Shared Equities-Arb Surface | completed | main | Task 5: Replace MakerV4 API Metadata, Strategy-Scoped Params, Signal Payload, And Readiness Contracts | `fluxboard/api.ts`, `fluxboard/components/domain/signal`, `fluxboard/config`, `fluxboard/types.ts`, `fluxboard/Params.tsx`, `fluxboard/stores.ts`, `fluxboard/tests/signal`, `fluxboard/__tests__`, `fluxboard/api.flux.test.ts`, `fluxboard/Signal.delta-pass-through.test.tsx`, `fluxboard/vite.config.ts` | `lanes/task-6-fluxboard` | `.worktrees/task-6-fluxboard` | `e8e34737ea`, `e13854360c`, `6a6d2dd9e5`, `3943dd689a`, `1e04ff1f33`, `350d0d4fe5`, `e0a7512ddf`, `7ef3c94c30`, `765554b919` | `pnpm --dir /home/ubuntu/nautilus_trader/.worktrees/makerv4-split-dual-arb-impl-20260317/fluxboard exec vitest run tests/signal/EquitiesArbSignalTable.test.tsx tests/signal/SignalFamilyFilter.test.tsx api.flux.test.ts Signal.delta-pass-through.test.tsx components/domain/signal/SignalTable.store.test.ts` pass (`5` files, `65` tests); `pnpm --dir /home/ubuntu/nautilus_trader/.worktrees/makerv4-split-dual-arb-impl-20260317/fluxboard exec vitest run __tests__/components/ParamsProfileColumns.test.tsx __tests__/Params.short-headers.test.tsx __tests__/config/paramsProfiles.test.ts` pass (`3` files, `27` tests) | 2026-03-19 UTC Task 6 lane opened on branch `lanes/task-6-fluxboard`; dedicated worktree provisioned. The fresh implementer confirmed the red phase with only Task 6 test-file edits in place, then stalled without starting production changes, so the controller reclaimed local TDD execution in the same lane worktree and landed the green slice in `7c6359b8fb`. Spec review then found three follow-up gaps: the shared table dropped required hedge-policy/fee/quote-health observability, `Trading` status semantics regressed, and default Vitest path excludes still hid `components/domain/signal/SignalTable.store.test.ts` plus `__tests__/components/ParamsProfileColumns.test.tsx`; the controller fixed those gaps in `fa1098fd16`, spec re-review then found one remaining `/equities/params` issue because the profile selector still exposed non-equities legacy options, the controller fixed that final gap in `a73e97978d`, and spec re-review passed with no findings. Quality review then found three follow-up risks: the shared equities table needed a mixed-rollout fallback from `maker_v4` to `equities_arb`, metadata-only taker rows needed the same family derivation as `SignalTable.tsx`, and split-family params schema selection needed deterministic row ordering; those were fixed and rerun in `f36630b37c`. Later quality loops found additional mixed-rollout gaps in legacy signal passthrough, params family recovery, schema selection, hidden-key filtering, legacy `applies_to` aliases, and top-level family authority; those were reproduced with focused red tests and fixed across `2c1a489885`, `91e11d0429`, `1b3f7ed9c9`, `9c7371c163`, and `a22dc967a9`. A fresh quality re-review on the full Task 6 diff from `f8fcda29d1..a22dc967a9` found no findings, the approved lane series was integrated onto the shared branch as commits `e8e34737ea` through `765554b919`, and the exact Task 6 verification bundles were rerun successfully in the shared worktree after integration |
| Task 7: Update Deploy, Portfolio, And Readiness Contracts For Dual Strategies Per Asset | in_progress | main | Task 5: Replace MakerV4 API Metadata, Strategy-Scoped Params, Signal Payload, And Readiness Contracts | `deploy/equities`, `deploy/equities/systemd`, `ops/scripts/deploy/equities_stack.sh`, `ops/scripts/deploy/install_equities_systemd.sh`, `systems/flux/flux/runners/equities/run_api.py`, `systems/flux/flux/runners/equities/run_node.py`, `systems/flux/flux/runners/equities/run_portfolio.py`, `systems/flux/flux/runners/equities/readiness.py`, `systems/flux/flux/api/app.py`, `systems/flux/flux/strategies/shared/equities_arb`, `systems/flux/flux/strategies/makerv3/publisher.py`, `systems/flux/flux/strategies/equities_maker`, `systems/flux/flux/strategies/equities_taker`, `tests/unit_tests/examples/strategies`, `tests/unit_tests/flux/api/test_app.py`, `tests/unit_tests/flux/api/test_equities_profile_contract.py` | `lanes/task-7-deploy-readiness` | `.worktrees/task-7-deploy-readiness` | `9b5919fd8f`, `d7fab0681b` | Red verified in lane after `uv sync --all-groups --all-extras`: `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 uv run --group test pytest tests/unit_tests/examples/strategies/test_equities_stack_contract.py::test_equities_live_config_allows_dual_strategy_ids_for_same_portfolio_asset -q` fail; `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 uv run --group test pytest tests/unit_tests/examples/strategies/test_equities_stack_contract.py::test_equities_dual_variant_strategy_files_preserve_shared_session_contract -q` fail; `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 uv run --group test pytest tests/unit_tests/examples/strategies/test_equities_readiness.py::test_evaluate_equities_readiness_requires_both_same_asset_strategy_ids -q` pass; `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 uv run --group test pytest tests/unit_tests/examples/strategies/test_equities_run_portfolio.py::test_strategy_ids_by_asset_groups_distinct_same_asset_variants -q` pass; `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 uv run --group test pytest tests/unit_tests/examples/strategies/test_equities_run_api.py::test_resolve_strategy_name_accepts_split_equities_defaults tests/unit_tests/examples/strategies/test_equities_run_api.py::test_build_profile_strategy_maps_reads_core_prod_allowlist_from_shared_live_config tests/unit_tests/examples/strategies/test_equities_run_api.py::test_main_binds_per_strategy_metadata_from_root_strategy_contracts -q` fail (`3` tests); `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 uv run --group test pytest tests/unit_tests/examples/strategies/test_equities_run_node.py::test_build_node_keeps_shared_execution_claims_for_primary_same_asset_variant tests/unit_tests/examples/strategies/test_equities_run_node.py::test_build_node_disables_shared_execution_claims_for_secondary_same_asset_variant tests/unit_tests/examples/strategies/test_equities_run_portfolio.py::test_equities_portfolio_aggregator_deduplicates_secondary_same_asset_components_and_positions tests/unit_tests/flux/api/test_app.py::test_param_schema_endpoint_rejects_profile_scoped_mixed_family_equities_request_without_strategy tests/unit_tests/flux/api/test_app.py::test_single_params_update_rejects_profile_scoped_mixed_family_equities_request_without_strategy -q` fail (`4` tests). Green verified in lane on `d7fab0681b`: `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 uv run --group test pytest tests/unit_tests/examples/strategies/test_equities_run_portfolio.py tests/unit_tests/examples/strategies/test_equities_stack_contract.py tests/unit_tests/examples/strategies/test_equities_readiness.py -q` pass (`69` tests); `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 uv run --group test pytest tests/unit_tests/examples/strategies/test_equities_run_api.py tests/unit_tests/examples/strategies/test_equities_run_node.py tests/unit_tests/flux/api/test_app.py tests/unit_tests/flux/api/test_equities_profile_contract.py -q` pass (`189` tests); `git diff --check` pass | 2026-03-19 UTC Task 7 lane opened from shared head `b503f7f17f` on branch `lanes/task-7-deploy-readiness`. The initial fresh implementer stalled before the red-test phase, so the controller reclaimed local TDD execution in the same lane worktree. After seeding the lane env with `uv sync --all-groups --all-extras`, the controller reproduced the real contract gap: deploy config and strategy template still expose single-family `*_makerv4` / local-risk semantics, while the same-asset `run_portfolio.py` grouping and readiness expectation tests already pass unchanged. An additional controller-owned red slice pinned the high-risk `run_api.main()` seam. The green slice then landed in `9b5919fd8f`: split maker/taker live allowlists and `strategy_contracts`, generated per-variant strategy TOMLs and checked-in service manifests, updated deploy docs/contracts, promoted the shared API bootstrap default to `equities_maker`, and taught `run_api.py` to resolve split-family defaults plus runtime params. A fresh spec review on `b503f7f17f..9b5919fd8f` found no findings. A fresh quality review on the same diff then reopened Task 7 with three real gaps: secondary same-asset nodes still claim/reconcile shared venue orders, shared-position portfolio and balances paths still double-count duplicate maker+taker payloads for the same asset, and profile-scoped `/api/v1/param-schema` / single-target params updates still silently resolve to the first family in a mixed equities profile. The controller reproduced each gap with failing tests, fixed the ownership, aggregation, and mixed-family params contract seams in `d7fab0681b`, and reran the expanded Task 7 verification bundle successfully. A fresh spec re-review on `b503f7f17f..d7fab0681b` then found two blockers in that fix: the new primary-owner path drops secondary same-asset inventory from the shared portfolio instead of preserving shared asset-level exposure, and it reintroduces hidden cross-strategy ownership by disabling secondary external-order claims and reconciliation. Task 7 is reopened for a non-ownership correction that removes the primary-owner model while preserving the mixed-family params ambiguity guard. |
| Task 8: Retire Legacy MakerV4 Equities Control-Plane Contract And Run Final Verification | not_started | unassigned | Task 6: Update Fluxboard To A Shared Equities-Arb Surface, Task 7: Update Deploy, Portfolio, And Readiness Contracts For Dual Strategies Per Asset | `deploy/equities`, `fluxboard`, `tests`, `docs/plans` | `shared` | `shared` | none | not_run | Plan created |

---

### Task 0: Align Approved `equities_maker` / `equities_taker` Naming

**Files:**
- Modify: `docs/plans/2026-03-17-equities-makerv4-split-design.md`
- Modify: `docs/plans/2026-03-17-equities-makerv4-split.md`
- Modify: `GitHub PR #64 title/body`

**Dependencies:** `Task 1: Lock Split Contract In Docs And Execution Matrix`

**Write Scope:** `docs/plans/2026-03-17-equities-makerv4-split-design.md`, `docs/plans/2026-03-17-equities-makerv4-split.md`, `GitHub PR #64 title/body`

**Verification Commands:**
- `git diff --check`

**Step 1: Replace stale split-family naming in the execution source of truth**

Update the implementation plan and design doc so the approved naming contract is used consistently:

- `equities_make_take` -> `equities_maker`
- `equities_take_take` -> `equities_taker`
- `<symbol>_tradexyz_make_take` -> `<symbol>_tradexyz_maker`
- `<symbol>_tradexyz_take_take` -> `<symbol>_tradexyz_taker`
- operator labels `Make-Take` / `Take-Take` -> `Maker` / `Taker`
- strategy family / param set / class examples align to the new names

Preserve the already-approved product semantics exactly while making the naming contract current.

**Step 2: Align downstream task text**

Update later task titles, file paths, verification commands, fixture examples, and prose references so Tasks 3 through 8 consistently target `equities_maker` / `equities_taker` and the `maker` / `taker` strategy IDs.

**Step 3: Update PR metadata**

Update draft implementation PR 64 title/body so the implementation lane matches the approved naming contract without changing its stacked-PR semantics.

**Step 4: Run doc hygiene**

Run:

```bash
git diff --check
```

Expected: PASS.

**Step 5: Commit**

```bash
git add \
  docs/plans/2026-03-17-equities-makerv4-split-design.md \
  docs/plans/2026-03-17-equities-makerv4-split.md
git commit -m "docs: align maker taker naming"
```

**Step 6: Update the Progress Tracker**

Mark Task 0 complete after the naming alignment, PR metadata update, and verification are recorded.

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 1: Lock Split Contract In Docs And Execution Matrix

**Files:**
- Modify: `docs/plans/2026-03-17-equities-makerv4-split-design.md`
- Modify: `deploy/equities/README.md`
- Modify: `fluxboard/docs/equities_contract.md`
- Modify: `deploy/equities/strategies/README.md`
- Modify: `docs/plans/2026-03-17-equities-makerv4-split.md`

**Dependencies:** `none`

**Write Scope:** `docs/plans/2026-03-17-equities-makerv4-split-design.md`, `deploy/equities/README.md`, `fluxboard/docs/equities_contract.md`, `deploy/equities/strategies/README.md`, `docs/plans/2026-03-17-equities-makerv4-split.md`

**Verification Commands:**
- `git diff --check`

**Step 1: Freeze the contract in docs**

Document all of the following explicitly:

- `makerv4` is replaced, not preserved as the live equities family
- `maker` and `taker` can both run for the same symbol
- both share the same equities portfolio/book and asset-level risk view
- the exact current RTH / outside-RTH hedge semantics are preserved in wave 1
- local inventory/risk ownership knobs such as `des_qty_local`, `max_qty_local`, and `max_skew_bps_local` do not survive into the split families
- `/equities/params` uses separate params schemas for maker and taker selected from the strategy dropdown, not one blended contract
- `taker` is defined as a taker-on-both-venues strategy family
- no cross-strategy arbitration is part of this wave

**Step 2: Lock the execution matrix instead of landing a red shared branch**

Update this implementation plan so each later task owns the executable tests for its seam:

- Task 2 owns shared-core plus runner/profile-account extraction tests
- Tasks 3 and 4 own the registry and `run_node.py` launch-path coverage for the new families
- Task 5 owns both equities-specific API tests and the generic `flux.api.app` params-endpoint coverage, including mixed-family reads/writes
- Task 6 owns Fluxboard snapshot rendering plus websocket delta-merge coverage
- Task 7 owns deploy/readiness contract tests and should only touch `run_portfolio.py` if those tests expose a real remaining gap
- Task 8 owns the full end-to-end regression bundle and live-contract cleanup assertions

Committed work should keep the shared lane green. Local fail-first testing inside each task is still encouraged, but the plan should not rely on checking in deliberately failing shared-branch tests before the owning implementation lands.

**Step 3: Run doc hygiene**

Run:

```bash
git diff --check
```

Expected: PASS.

**Step 4: Commit the locked contract docs and execution matrix**

```bash
git add \
  docs/plans/2026-03-17-equities-makerv4-split-design.md \
  deploy/equities/README.md \
  fluxboard/docs/equities_contract.md \
  deploy/equities/strategies/README.md \
  docs/plans/2026-03-17-equities-makerv4-split.md
git commit -m "docs: lock equities split contract"
```

**Step 5: Update the Progress Tracker**

Mark Task 1 complete after the contract docs and execution-matrix updates are committed.

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 2: Extract Shared Equities-Arb Core From MakerV4

**Files:**
- Create: `systems/flux/flux/strategies/shared/equities_arb/__init__.py`
- Create: `systems/flux/flux/strategies/shared/equities_arb/core.py`
- Create: `systems/flux/flux/strategies/shared/equities_arb/instruments.py`
- Create: `systems/flux/flux/strategies/shared/equities_arb/hedging.py`
- Create: `systems/flux/flux/strategies/shared/equities_arb/observability.py`
- Create: `systems/flux/flux/strategies/shared/equities_arb/reference_balances.py`
- Modify: `systems/flux/flux/runners/equities/run_node.py`
- Modify: `systems/flux/flux/runners/shared/profile_accounts.py`
- Create: `tests/unit_tests/flux/strategies/shared/test_equities_arb_core.py`
- Modify: `systems/flux/flux/strategies/makerv4/fees.py`
- Modify: `systems/flux/flux/strategies/makerv4/instruments.py`
- Modify: `systems/flux/flux/strategies/makerv4/pricing.py`
- Modify: `systems/flux/flux/strategies/makerv4/strategy.py`
- Modify: `systems/flux/flux/strategies/makerv4/managed_orders.py`
- Modify: `systems/flux/flux/strategies/makerv4/publisher.py`
- Modify: `systems/flux/flux/strategies/makerv4/reference_balances.py`
- Modify: `tests/unit_tests/examples/strategies/test_equities_run_node.py`
- Modify: `tests/unit_tests/examples/strategies/test_equities_run_portfolio.py`
- Modify: `tests/unit_tests/flux/strategies/makerv4/test_instruments.py`
- Modify: `tests/unit_tests/flux/strategies/makerv4/test_pricing.py`
- Modify: `tests/unit_tests/flux/strategies/makerv4/test_reference_balances.py`

**Dependencies:** `Task 1: Lock Split Contract In Docs And Execution Matrix`

**Write Scope:** `systems/flux/flux/strategies/shared/equities_arb`, `systems/flux/flux/runners/equities/run_node.py`, `systems/flux/flux/runners/shared/profile_accounts.py`, `tests/unit_tests/flux/strategies/shared/test_equities_arb_core.py`, `systems/flux/flux/strategies/makerv4/fees.py`, `systems/flux/flux/strategies/makerv4/instruments.py`, `systems/flux/flux/strategies/makerv4/pricing.py`, `systems/flux/flux/strategies/makerv4/strategy.py`, `systems/flux/flux/strategies/makerv4/managed_orders.py`, `systems/flux/flux/strategies/makerv4/publisher.py`, `systems/flux/flux/strategies/makerv4/reference_balances.py`, `tests/unit_tests/examples/strategies/test_equities_run_node.py`, `tests/unit_tests/examples/strategies/test_equities_run_portfolio.py`, `tests/unit_tests/flux/strategies/makerv4/test_instruments.py`, `tests/unit_tests/flux/strategies/makerv4/test_pricing.py`, `tests/unit_tests/flux/strategies/makerv4/test_reference_balances.py`

**Verification Commands:**
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 uv run --group test pytest tests/unit_tests/flux/strategies/shared/test_equities_arb_core.py -q`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 uv run --group test pytest tests/unit_tests/flux/strategies/makerv4/test_strategy.py tests/unit_tests/flux/strategies/makerv4/test_publisher_contract.py tests/unit_tests/flux/strategies/makerv4/test_instruments.py tests/unit_tests/flux/strategies/makerv4/test_pricing.py tests/unit_tests/flux/strategies/makerv4/test_reference_balances.py tests/unit_tests/examples/strategies/test_equities_run_node.py tests/unit_tests/examples/strategies/test_equities_run_portfolio.py -q`

**Step 1: Write failing shared-core tests**

Add tests that pin reusable behavior for:

- shared fee assumptions payload
- shared fee-rule and fee-aware pricing helpers currently living in `makerv4.fees` and `makerv4.pricing`
- shared hedge policy / pending hedge payload shape
- shared backlog payload shape
- shared quote snapshot assembly for equities arb legs
- shared session-aware hedge policy behavior used by both families
- runner-facing instrument/reference-balance helpers that must stop being `makerv4`-owned
- runner capability seams that stop `run_node.py` from branching on `param_set == "makerv4"` for runtime params, immediate-hedge support, venue promotion, or allowed-instrument selection

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
- fee assumptions payload helpers and fee-rule resolution
- shared leg/quote snapshot assembly and quote/pricing helpers
- session-aware hedge policy helpers
- instrument mapping and reference-balance helpers used by the equities runner
- runner-facing strategy capability helpers used by `run_node.py`
- runner and shared-profile-account imports moved off `flux.strategies.makerv4.*` onto the shared equities-arb seam

Do not rename the live family yet. This task is only about isolating the reusable seam.

**Step 4: Re-run the shared-core and MakerV4 regression slice**

Expected: PASS with no behavior drift.

**Step 5: Commit**

```bash
git add \
  systems/flux/flux/strategies/shared/equities_arb \
  systems/flux/flux/runners/equities/run_node.py \
  systems/flux/flux/runners/shared/profile_accounts.py \
  tests/unit_tests/flux/strategies/shared/test_equities_arb_core.py \
  systems/flux/flux/strategies/makerv4/fees.py \
  systems/flux/flux/strategies/makerv4/instruments.py \
  systems/flux/flux/strategies/makerv4/pricing.py \
  systems/flux/flux/strategies/makerv4/strategy.py \
  systems/flux/flux/strategies/makerv4/managed_orders.py \
  systems/flux/flux/strategies/makerv4/publisher.py \
  systems/flux/flux/strategies/makerv4/reference_balances.py \
  tests/unit_tests/examples/strategies/test_equities_run_node.py \
  tests/unit_tests/examples/strategies/test_equities_run_portfolio.py \
  tests/unit_tests/flux/strategies/makerv4/test_instruments.py \
  tests/unit_tests/flux/strategies/makerv4/test_pricing.py \
  tests/unit_tests/flux/strategies/makerv4/test_reference_balances.py
git commit -m "refactor: extract shared equities arb core"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 3: Add `equities_maker` Strategy Family

**Files:**
- Create: `systems/flux/flux/strategies/equities_maker/__init__.py`
- Create: `systems/flux/flux/strategies/equities_maker/constants.py`
- Create: `systems/flux/flux/strategies/equities_maker/runtime_params.py`
- Create: `systems/flux/flux/strategies/equities_maker/strategy.py`
- Create: `tests/unit_tests/flux/strategies/equities_maker/test_runtime_params.py`
- Create: `tests/unit_tests/flux/strategies/equities_maker/test_strategy.py`
- Modify: `systems/flux/flux/strategies/registry.py`
- Modify: `systems/flux/flux/strategies/__init__.py`
- Modify: `systems/flux/flux/runners/equities/run_node.py`
- Modify: `tests/unit_tests/examples/strategies/test_equities_run_node.py`

**Dependencies:** `Task 2: Extract Shared Equities-Arb Core From MakerV4`

**Write Scope:** `systems/flux/flux/strategies/equities_maker`, `systems/flux/flux/strategies/registry.py`, `systems/flux/flux/strategies/__init__.py`, `systems/flux/flux/runners/equities/run_node.py`, `tests/unit_tests/flux/strategies/equities_maker/test_runtime_params.py`, `tests/unit_tests/flux/strategies/equities_maker/test_strategy.py`, `tests/unit_tests/examples/strategies/test_equities_run_node.py`

**Verification Commands:**
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 uv run --group test pytest tests/unit_tests/flux/strategies/equities_maker/test_runtime_params.py tests/unit_tests/flux/strategies/equities_maker/test_strategy.py -q`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 uv run --group test pytest tests/unit_tests/examples/strategies/test_equities_run_node.py -q`

**Step 1: Write the failing `maker` family tests**

Pin:

- registry identity
- suffix-based strategy-id resolution for `<symbol>_tradexyz_maker`
- runtime param surface without `taker`-only knobs or local inventory/risk ownership knobs
- exact preservation of the current RTH / outside-RTH hedge semantics
- maker quote lifecycle uses the shared equities-arb core
- shared portfolio/book risk is still the only asset-level risk source
- `run_node.py` resolves the `equities_maker` runtime params/config surface without falling back through `makerv4`-specific branches

**Step 2: Run tests to verify failure**

Expected: FAIL because `equities_maker` does not exist.

**Step 3: Implement the relatively thin `maker` family**

Create `EquitiesMakerStrategy` and config/runtime modules that:

- consume the shared core
- expose only the common plus maker-specific runtime params
- keep the existing session-aware hedge contract unchanged
- omit `des_qty_local`, `max_qty_local`, `max_skew_bps_local`, and equivalent local-inventory controls
- preserve current maker-side quote behavior
- replace the remaining `makerv4`-specific `run_node.py` wiring for runtime params, allowed instruments, immediate-hedge capability, and config construction with spec-driven `equities_maker` behavior

**Step 4: Re-run the focused `maker` tests**

Expected: PASS.

**Step 5: Commit**

```bash
git add \
  systems/flux/flux/strategies/equities_maker \
  systems/flux/flux/strategies/registry.py \
  systems/flux/flux/strategies/__init__.py \
  systems/flux/flux/runners/equities/run_node.py \
  tests/unit_tests/flux/strategies/equities_maker/test_runtime_params.py \
  tests/unit_tests/flux/strategies/equities_maker/test_strategy.py \
  tests/unit_tests/examples/strategies/test_equities_run_node.py
git commit -m "feat: add equities maker strategy family"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 4: Add `equities_taker` Strategy Family

**Files:**
- Create: `systems/flux/flux/strategies/equities_taker/__init__.py`
- Create: `systems/flux/flux/strategies/equities_taker/constants.py`
- Create: `systems/flux/flux/strategies/equities_taker/runtime_params.py`
- Create: `systems/flux/flux/strategies/equities_taker/strategy.py`
- Create: `tests/unit_tests/flux/strategies/equities_taker/test_runtime_params.py`
- Create: `tests/unit_tests/flux/strategies/equities_taker/test_strategy.py`
- Modify: `systems/flux/flux/strategies/registry.py`
- Modify: `systems/flux/flux/strategies/__init__.py`
- Modify: `systems/flux/flux/runners/equities/run_node.py`
- Modify: `tests/unit_tests/examples/strategies/test_equities_run_node.py`

**Dependencies:** `Task 2: Extract Shared Equities-Arb Core From MakerV4`

**Write Scope:** `systems/flux/flux/strategies/equities_taker`, `systems/flux/flux/strategies/registry.py`, `systems/flux/flux/strategies/__init__.py`, `systems/flux/flux/runners/equities/run_node.py`, `tests/unit_tests/flux/strategies/equities_taker/test_runtime_params.py`, `tests/unit_tests/flux/strategies/equities_taker/test_strategy.py`, `tests/unit_tests/examples/strategies/test_equities_run_node.py`

**Verification Commands:**
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 uv run --group test pytest tests/unit_tests/flux/strategies/equities_taker/test_runtime_params.py tests/unit_tests/flux/strategies/equities_taker/test_strategy.py -q`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 uv run --group test pytest tests/unit_tests/examples/strategies/test_equities_run_node.py -q`

**Step 1: Write the failing `taker` family tests**

Pin:

- registry identity
- suffix-based strategy-id resolution for `<symbol>_tradexyz_taker`
- runtime params keep only taker-relevant knobs and omit local inventory/risk ownership knobs
- `taker` signal generation and execution are family-owned, not hidden behind `execution_mode`
- `taker` is pinned as a taker-on-both-venues strategy rather than a maker quote loop
- exact preservation of the current RTH / outside-RTH hedge semantics
- hedge backlog and shared portfolio-risk reads still work through the shared core
- `run_node.py` resolves the `equities_taker` runtime params/config surface without relying on `makerv4`-specific fallback paths

**Step 2: Run tests to verify failure**

Expected: FAIL because `equities_taker` does not exist.

**Step 3: Implement the extracted `taker` family**

Create `EquitiesTakerStrategy` and config/runtime modules that:

- consume the shared core
- expose taker-specific params such as threshold and cooldown knobs
- keep the current aggressive outside-band behavior and shared hedge path
- implement a taker-on-both-venues execution model instead of reusing the maker quote loop
- perform the non-trivial extraction currently hidden inside `makerv4.execution_mode` branches
- omit `des_qty_local`, `max_qty_local`, `max_skew_bps_local`, and equivalent local-inventory controls
- replace the remaining `makerv4`-specific `run_node.py` wiring for runtime params, allowed instruments, immediate-hedge capability, and config construction with spec-driven `equities_taker` behavior

**Step 4: Re-run the focused `taker` tests**

Expected: PASS.

**Step 5: Commit**

```bash
git add \
  systems/flux/flux/strategies/equities_taker \
  systems/flux/flux/strategies/registry.py \
  systems/flux/flux/strategies/__init__.py \
  systems/flux/flux/runners/equities/run_node.py \
  tests/unit_tests/flux/strategies/equities_taker/test_runtime_params.py \
  tests/unit_tests/flux/strategies/equities_taker/test_strategy.py \
  tests/unit_tests/examples/strategies/test_equities_run_node.py
git commit -m "feat: add equities taker strategy family"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 5: Replace MakerV4 API Metadata, Strategy-Scoped Params, Signal Payload, And Readiness Contracts

**Files:**
- Modify: `systems/flux/flux/api/app.py`
- Modify: `systems/flux/flux/runners/equities/run_api.py`
- Modify: `systems/flux/flux/runners/equities/run_bridge.py`
- Modify: `systems/flux/flux/runners/equities/readiness.py`
- Modify: `systems/flux/flux/api/_payloads_signals.py`
- Modify: `fluxboard/types.ts`
- Modify: `tests/unit_tests/flux/api/test_app.py`
- Modify: `tests/unit_tests/examples/strategies/test_equities_run_api.py`
- Modify: `tests/unit_tests/examples/strategies/test_equities_run_bridge.py`
- Modify: `tests/unit_tests/examples/strategies/test_equities_readiness.py`
- Modify: `tests/unit_tests/flux/api/test_equities_profile_contract.py`
- Modify: `tests/unit_tests/flux/api/test_payloads.py`

**Dependencies:** `Task 3: Add \`equities_maker\` Strategy Family`, `Task 4: Add \`equities_taker\` Strategy Family`

**Write Scope:** `systems/flux/flux/api/app.py`, `systems/flux/flux/runners/equities/run_api.py`, `systems/flux/flux/runners/equities/run_bridge.py`, `systems/flux/flux/runners/equities/readiness.py`, `systems/flux/flux/api/_payloads_signals.py`, `fluxboard/types.ts`, `tests/unit_tests/flux/api/test_app.py`, `tests/unit_tests/examples/strategies/test_equities_run_api.py`, `tests/unit_tests/examples/strategies/test_equities_run_bridge.py`, `tests/unit_tests/examples/strategies/test_equities_readiness.py`, `tests/unit_tests/flux/api/test_equities_profile_contract.py`, `tests/unit_tests/flux/api/test_payloads.py`

**Verification Commands:**
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 uv run --group test pytest tests/unit_tests/flux/api/test_app.py tests/unit_tests/examples/strategies/test_equities_run_api.py tests/unit_tests/examples/strategies/test_equities_run_bridge.py tests/unit_tests/examples/strategies/test_equities_readiness.py tests/unit_tests/flux/api/test_equities_profile_contract.py tests/unit_tests/flux/api/test_payloads.py -q`

**Step 1: Write the failing API and payload tests**

Require:

- strategy metadata recognizes `aapl_tradexyz_maker` and `aapl_tradexyz_taker`
- the params API can serve separate make-take and take-take `params_schema`, `params_defaults`, and `param_set` contracts keyed off the selected strategy/family, for example via `GET /api/v1/param-schema?strategy=<strategy_id>`, rather than a single global equities schema
- `GET /api/v1/params` can load mixed make-take and take-take rows in one response without one family rejecting the other family's keys
- bulk `POST/PATCH /api/v1/params` accepts mixed-family updates in one request and validates each strategy against its own param contract
- `GET/POST/PATCH /api/v1/strategies/<strategy_id>/parameters` resolves schema/defaults/validation from the selected strategy family instead of the app-wide default bundle
- signal payloads expose one shared equities-arb operator contract
- fee assumptions and pricing fields remain visible for both variants
- family-specific rows still share common leg/quote-health semantics
- bridge allowlist and explicit strategy-id resolution accept both variants cleanly
- readiness quote-snapshot parsing works against the new shared signal contract instead of hard-coding `payload["maker_v4"]`
- the production `run_api.main()` path binds per-strategy asset/family metadata from merged config and `strategy_contracts`, not only helper tests that inject contracts into an isolated `api_cfg`

**Step 2: Run tests to verify failure**

Expected: FAIL because the API currently recognizes `maker_v4`-specific semantics.

**Step 3: Update metadata and payload builders**

Implement:

- strategy-aware params schema/default selection in `flux.api.app` and `run_api.py`, exposed through an explicit strategy-scoped selector rather than only a page-global profile query
- per-strategy schema/default/param-set resolution for all params read/write endpoints, including mixed-family `/api/v1/params` loads and bulk updates in the generic app surface
- new strategy-id-to-spec resolution
- family-aware metadata emission
- `run_api.main()` and related helpers wired so the live merged-config path can resolve `strategy_contracts` and family metadata without relying on a synthetic api-only test shape
- shared equities-arb payload shape replacing the hard-coded MakerV4 contract
- readiness payload parsing updated in the same task so the API/signal contract change cannot break live gating between commits

**Step 4: Re-run the focused API suite**

Expected: PASS.

**Step 5: Commit**

```bash
git add \
  systems/flux/flux/api/app.py \
  systems/flux/flux/runners/equities/run_api.py \
  systems/flux/flux/runners/equities/run_bridge.py \
  systems/flux/flux/runners/equities/readiness.py \
  systems/flux/flux/api/_payloads_signals.py \
  fluxboard/types.ts \
  tests/unit_tests/flux/api/test_app.py \
  tests/unit_tests/examples/strategies/test_equities_run_api.py \
  tests/unit_tests/examples/strategies/test_equities_run_bridge.py \
  tests/unit_tests/examples/strategies/test_equities_readiness.py \
  tests/unit_tests/flux/api/test_equities_profile_contract.py \
  tests/unit_tests/flux/api/test_payloads.py
git commit -m "feat: add dual equities arb api contract"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 6: Update Fluxboard To A Shared Equities-Arb Surface

**Files:**
- Modify: `fluxboard/api.ts`
- Create: `fluxboard/components/domain/signal/EquitiesArbSignalTable.tsx`
- Modify: `fluxboard/components/domain/signal/SignalTable.tsx`
- Modify: `fluxboard/Params.tsx`
- Modify: `fluxboard/config/paramsProfiles.ts`
- Modify: `fluxboard/components/panels/ParamsPanel.tsx`
- Modify: `fluxboard/stores.ts`
- Modify: `fluxboard/api.flux.test.ts`
- Modify: `fluxboard/Signal.delta-pass-through.test.tsx`
- Modify: `fluxboard/tests/signal/MakerV4SignalTable.test.tsx`
- Create: `fluxboard/tests/signal/EquitiesArbSignalTable.test.tsx`
- Modify: `fluxboard/tests/signal/SignalFamilyFilter.test.tsx`
- Modify: `fluxboard/components/domain/signal/SignalTable.store.test.ts`
- Modify: `fluxboard/__tests__/components/ParamsProfileColumns.test.tsx`
- Modify: `fluxboard/__tests__/Params.short-headers.test.tsx`
- Modify: `fluxboard/__tests__/config/paramsProfiles.test.ts`

**Dependencies:** `Task 5: Replace MakerV4 API Metadata, Strategy-Scoped Params, Signal Payload, And Readiness Contracts`

**Write Scope:** `fluxboard/api.ts`, `fluxboard/components/domain/signal/EquitiesArbSignalTable.tsx`, `fluxboard/components/domain/signal/SignalTable.tsx`, `fluxboard/Params.tsx`, `fluxboard/config/paramsProfiles.ts`, `fluxboard/components/panels/ParamsPanel.tsx`, `fluxboard/stores.ts`, `fluxboard/api.flux.test.ts`, `fluxboard/Signal.delta-pass-through.test.tsx`, `fluxboard/tests/signal/EquitiesArbSignalTable.test.tsx`, `fluxboard/tests/signal/MakerV4SignalTable.test.tsx`, `fluxboard/tests/signal/SignalFamilyFilter.test.tsx`, `fluxboard/components/domain/signal/SignalTable.store.test.ts`, `fluxboard/__tests__/components/ParamsProfileColumns.test.tsx`, `fluxboard/__tests__/Params.short-headers.test.tsx`, `fluxboard/__tests__/config/paramsProfiles.test.ts`

**Verification Commands:**
- `pnpm --dir fluxboard exec vitest run tests/signal/EquitiesArbSignalTable.test.tsx tests/signal/SignalFamilyFilter.test.tsx api.flux.test.ts Signal.delta-pass-through.test.tsx components/domain/signal/SignalTable.store.test.ts`
- `pnpm --dir fluxboard exec vitest run __tests__/components/ParamsProfileColumns.test.tsx __tests__/Params.short-headers.test.tsx __tests__/config/paramsProfiles.test.ts`

**Step 1: Write the failing Fluxboard tests**

Require:

- the equities signal route renders one shared table for both variants
- rows include a visible variant label
- the family filter and params profile logic no longer assume `maker_v4`
- rows sort/group by symbol then variant
- the params route uses separate make-take and take-take schemas chosen from the selected strategy/family in the dropdown
- the params route shows common controls first, then session/shared-risk controls, then family-specific controls for the active family
- local inventory/risk controls such as `des_qty_local`, `max_qty_local`, and `max_skew_bps_local` are absent from the split equities profiles
- API client metadata normalization still recognizes the split families and param sets
- websocket delta patches preserve the new shared equities-arb top-level keys so live rows keep updating after the initial snapshot
- persisted UI state migrates prior `maker_v4` prefs onto the new split profiles cleanly, with an explicit params-store version bump and migration path

**Step 2: Run tests to verify failure**

Expected: FAIL because the UI still routes equities through `MakerV4SignalTable` and `maker_v4` params assumptions.

**Step 3: Implement the shared equities-arb surface**

Build a shared table that:

- reuses the existing leg and operator affordances where possible
- adds explicit variant labeling
- keeps fee and hedge-policy observability visible for both variants
- updates Params profile routing, strategy-driven schema fetching/caching, canonical ordering, and persisted store migration for the shared `/equities/params` workflow
- updates the websocket delta-merge path so new shared-equities payload keys survive incremental patches instead of relying on full snapshot refresh
- bumps the persisted params-store version and maps legacy `maker_v4` active-profile/column-pref state onto the new split profiles

**Step 4: Re-run the focused Fluxboard suite**

Expected: PASS.

**Step 5: Commit**

```bash
git add \
  fluxboard/api.ts \
  fluxboard/components/domain/signal/EquitiesArbSignalTable.tsx \
  fluxboard/components/domain/signal/SignalTable.tsx \
  fluxboard/Params.tsx \
  fluxboard/config/paramsProfiles.ts \
  fluxboard/components/panels/ParamsPanel.tsx \
  fluxboard/stores.ts \
  fluxboard/api.flux.test.ts \
  fluxboard/Signal.delta-pass-through.test.tsx \
  fluxboard/tests/signal/EquitiesArbSignalTable.test.tsx \
  fluxboard/tests/signal/MakerV4SignalTable.test.tsx \
  fluxboard/tests/signal/SignalFamilyFilter.test.tsx \
  fluxboard/components/domain/signal/SignalTable.store.test.ts \
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
- Modify if needed after contract tests expose a real gap: `systems/flux/flux/runners/equities/run_portfolio.py`
- Modify: `systems/flux/flux/runners/equities/readiness.py`
- Modify: `tests/unit_tests/examples/strategies/test_equities_run_portfolio.py`
- Modify: `tests/unit_tests/examples/strategies/test_equities_stack_contract.py`
- Modify: `tests/unit_tests/examples/strategies/test_equities_readiness.py`

**Dependencies:** `Task 5: Replace MakerV4 API Metadata, Strategy-Scoped Params, Signal Payload, And Readiness Contracts`

**Write Scope:** `deploy/equities/equities.live.toml`, `deploy/equities/README.md`, `deploy/equities/systemd/flux-equities.target`, `deploy/equities/systemd/flux-pulse.sudoers`, `deploy/equities/strategies/equities.strategy.template.toml`, `deploy/equities/strategies/*.toml`, `deploy/equities/strategies/README.md`, `ops/scripts/deploy/equities_stack.sh`, `ops/scripts/deploy/install_equities_systemd.sh`, `systems/flux/flux/runners/equities/run_portfolio.py`, `systems/flux/flux/runners/equities/readiness.py`, `tests/unit_tests/examples/strategies/test_equities_run_portfolio.py`, `tests/unit_tests/examples/strategies/test_equities_stack_contract.py`, `tests/unit_tests/examples/strategies/test_equities_readiness.py`

**Verification Commands:**
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 uv run --group test pytest tests/unit_tests/examples/strategies/test_equities_run_portfolio.py tests/unit_tests/examples/strategies/test_equities_stack_contract.py tests/unit_tests/examples/strategies/test_equities_readiness.py -q`

**Step 1: Write the failing deploy/readiness tests**

Require:

- two active strategy IDs per symbol are allowed in `equities.live.toml`
- `strategy_contracts` may repeat `portfolio_asset_id`
- `tests/unit_tests/examples/strategies/test_equities_run_portfolio.py` must include a true same-symbol fixture such as `aapl_tradexyz_maker` plus `aapl_tradexyz_taker` sharing `portfolio_asset_id="AAPL"`; duplicated copies of the same strategy ID do not count
- `tests/unit_tests/examples/strategies/test_equities_readiness.py` must include the same `maker` plus `taker` same-asset fixture and prove readiness expects both strategy IDs while evaluating one shared asset-level portfolio state
- `tests/unit_tests/examples/strategies/test_equities_stack_contract.py` must stop hard-rejecting duplicate `portfolio_asset_id` values and instead assert uniqueness at the strategy-id level while allowing repeated asset IDs for the split variants
- readiness expects both enrolled strategies while still checking shared asset-level portfolio health
- the template and README use the new naming pattern
- actual per-variant strategy TOMLs exist under the current one-strategy-per-node deploy model
- the split keeps the existing session contract, including `use_regular_trading_hours = false` and `outside_rth_hedge_enabled`, for both families
- templates and deploy profiles do not surface local inventory/risk ownership knobs
- stack/install scripts discover, install, and launch both variants per symbol without hand-edited service drift
- existing tuple-based `_strategy_ids_by_asset` grouping remains sufficient unless the deploy/readiness tests prove otherwise

**Step 2: Run tests to verify failure**

Expected: FAIL because the current stack contract still assumes one active strategy per asset.

**Step 3: Update deploy and portfolio/readiness logic**

Implement:

- dual-strategy allowlists
- dual `strategy_contracts` rows per asset
- actual per-variant strategy files replacing the live `makerv4` TOMLs
- reuse the existing tuple-based `run_portfolio.py` grouping and only patch portfolio-runner code if the deploy/readiness suite exposes a real remaining gap
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

### Task 8: Retire Legacy MakerV4 Equities Control-Plane Contract And Run Final Verification

**Files:**
- Modify: `deploy/equities/equities.live.toml`
- Modify: `deploy/equities/strategies/*.toml`
- Modify: `fluxboard/components/domain/signal/MakerV4SignalTable.tsx`
- Modify: `fluxboard/docs/equities_contract.md`
- Modify: `docs/plans/2026-03-17-equities-makerv4-split-design.md`
- Modify: `docs/plans/2026-03-17-equities-makerv4-split.md`

**Dependencies:** `Task 6: Update Fluxboard To A Shared Equities-Arb Surface`, `Task 7: Update Deploy, Portfolio, And Readiness Contracts For Dual Strategies Per Asset`

**Write Scope:** `deploy/equities`, `fluxboard/components/domain/signal/MakerV4SignalTable.tsx`, `fluxboard/docs/equities_contract.md`, `docs/plans/2026-03-17-equities-makerv4-split-design.md`, `docs/plans/2026-03-17-equities-makerv4-split.md`, `tests`

**Verification Commands:**
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 uv run --group test pytest tests/unit_tests/flux/strategies/shared/test_equities_arb_core.py tests/unit_tests/flux/strategies/equities_maker/test_runtime_params.py tests/unit_tests/flux/strategies/equities_maker/test_strategy.py tests/unit_tests/flux/strategies/equities_taker/test_runtime_params.py tests/unit_tests/flux/strategies/equities_taker/test_strategy.py tests/unit_tests/examples/strategies/test_equities_run_api.py tests/unit_tests/examples/strategies/test_equities_run_bridge.py tests/unit_tests/examples/strategies/test_equities_run_node.py tests/unit_tests/examples/strategies/test_equities_run_portfolio.py tests/unit_tests/examples/strategies/test_equities_stack_contract.py tests/unit_tests/examples/strategies/test_equities_readiness.py tests/unit_tests/flux/api/test_app.py tests/unit_tests/flux/api/test_equities_profile_contract.py tests/unit_tests/flux/api/test_payloads.py -q`
- `pnpm --dir fluxboard exec vitest run tests/signal/EquitiesArbSignalTable.test.tsx tests/signal/SignalFamilyFilter.test.tsx __tests__/components/ParamsProfileColumns.test.tsx __tests__/Params.short-headers.test.tsx __tests__/config/paramsProfiles.test.ts api.flux.test.ts Signal.delta-pass-through.test.tsx components/domain/signal/SignalTable.store.test.ts`
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
- stale family-specific assumptions that are no longer exercised by active equities control-plane paths
- the dedicated MakerV4 Fluxboard surface if it is no longer used by any active route

Do not delete dormant/internal `makerv4` strategy package code, registry identity, or compatibility tests in this wave unless they are proven unused after the shared-core extraction. The requirement here is to remove `makerv4` from active equities deploy/API/UI paths, not to force repo-wide historical cleanup.

**Step 4: Re-run the final verification bundle**

Expected: PASS.

**Step 5: Commit**

```bash
git add \
  deploy/equities \
  fluxboard/components/domain/signal/MakerV4SignalTable.tsx \
  fluxboard/docs/equities_contract.md \
  docs/plans/2026-03-17-equities-makerv4-split-design.md \
  docs/plans/2026-03-17-equities-makerv4-split.md \
  tests
git commit -m "feat: replace equities makerv4 with split arb families"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

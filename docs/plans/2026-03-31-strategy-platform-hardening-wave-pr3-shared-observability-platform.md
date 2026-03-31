# Strategy Platform Hardening Wave PR3 Shared Observability Platform Implementation Plan

> **For the execution agent:** REQUIRED SUB-SKILL: Before implementing this plan, choose exactly one execution mode and use the matching skill: `superpowers:subagent-driven-development` for same-session execution or `superpowers:executing-plans` for a separate-session handoff.
> **Progress:** The Progress Tracker in this document is the execution source of truth and must be updated on every state change.

**Goal:** Move strategy-generic observability helpers out of Makerv3 ownership, migrate all current consumers, and preserve topic names plus operator/API payload semantics through an explicit shared observability-contract namespace.

**Architecture:** Extract generic serialization, legacy topic constants, payload-schema helpers, alert publishing, JSON/event publishing, balance/trade helper logic, and related helpers into an explicit `strategies/shared/observability` ownership surface. Keep family-specific state builders local. This PR must remove the current `shared -> makerv3` and `makerv4 -> makerv3 publisher/constants` dependency edges without renaming external topics.

**Tech Stack:** Python strategy/runtime publishing code, shared helpers, API payload builders, pytest.

**Context Docs:**
- Design: `docs/plans/2026-03-31-strategy-platform-hardening-wave-design.md`
- PRD: `docs/prd/2026-03-31-strategy-platform-hardening-wave.md`
- Relevant specs/runbooks: `systems/flux/docs/makerv3.md`, `docs/plans/2026-03-31-strategy-platform-hardening-wave.md`

**Decision Summary:**
- preserve all current topic names in this wave
- extract helper ownership, not family-specific state construction
- migrate Makerv3, Makerv4, equities, and `shared/*` consumers in the same PR
- frozen external topic and payload-schema ownership should live under `observability/contracts`, not a generic top-level helper bucket

## Release Gates

| Environment | Required for merge? | Requirement |
| --- | --- | --- |
| `default-unit` | yes | shared, Makerv3, API payload, and realtime proof must pass |
| `ibkr-unit` | yes | `python -c "import ibapi"` must succeed and Makerv4 proof must pass |
| `pilot` | yes | deploy one pinned pilot release from the PR head and validate the smoke bundle below |

## Affected Pilot Surfaces

- Makerv3 and Makerv4 state, alert, trade, and balances publishers
- shared equities helper modules that publish observability payloads
- realtime, signals, and balances API consumers
- readiness and bridge consumers of frozen legacy topics

## Pilot Stacks To Move Together

- `tokenmm`
- `equities`
- `flux-api` if it is deployed as a separate pilot stack

## Pilot Deploy Units And Promotion Order

Deploy units:

- `flux.runners.tokenmm.run_node`
- `flux.runners.tokenmm.run_bridge`
- `flux.runners.equities.run_node`
- `flux.runners.equities.run_bridge`
- `flux.api.app`

Promotion order:

1. build one pinned pilot release from the exact PR head
2. deploy the tokenmm and equities runner/bridge units plus `flux.api.app` from that same release
3. validate frozen topic routing, payload decoding, and observability bootstrap
4. promote the same units together only after the smoke bundle is clean

## Pilot Smoke Bundle

1. Deploy the pilot release from the exact PR head.
2. Confirm legacy topic names remain unchanged for state, alert, balances, order-intent, and trade paths.
3. Confirm realtime and API payload consumers still decode the same keys and semantics.
4. Confirm Makerv4 and shared equities helpers no longer depend on MakerV3 publisher/constants paths.

## PR-Local Rollback

- revert the whole PR
- redeploy the previous pinned pilot release
- do not leave shared observability helpers live while reverting only one publishing family

## Progress Tracker

**Source of truth:** Update this table whenever task state changes. Do not rely on memory, chat history, or TodoWrite alone.

| Task | Status | Owner | Depends On | Write Scope | Lane Branch | Worktree Path | Commit / Diff | Verification | Notes / Last Update |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Overall | not_started | unassigned | none | `systems/flux/flux/strategies/shared`, `systems/flux/flux/strategies/makerv3`, `systems/flux/flux/strategies/makerv4`, `systems/flux/flux/strategies/equities_taker`, `systems/flux/flux/api`, `tests/unit_tests/flux/strategies`, `tests/unit_tests/flux/api`, `systems/flux/docs` | `wave/pr3-observability-platform` | `.worktrees/strategy-platform-pr3` | none | not_run | Plan created |
| Task 1: Lock topic and payload contracts in tests | not_started | unassigned | none | `tests/unit_tests/flux/test_architecture_boundaries.py`, `tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py`, `tests/unit_tests/flux/strategies/makerv4/test_observability_and_exports.py`, `tests/unit_tests/flux/strategies/shared/test_trades.py`, `tests/unit_tests/flux/strategies/shared/test_equities_arb_core.py`, `tests/unit_tests/flux/api/test_payloads.py`, `tests/unit_tests/flux/api/test_payload_snapshots.py`, `tests/unit_tests/flux/api/test_realtime_contract.py`, `tests/unit_tests/flux/runners/test_tokenmm_readiness.py`, `tests/unit_tests/examples/strategies/test_tokenmm_run_bridge.py`, `tests/unit_tests/examples/strategies/test_equities_run_bridge.py` | `wave/pr3-observability-platform` | `.worktrees/strategy-platform-pr3` | none | not_run | Plan created |
| Task 2: Extract shared observability modules | not_started | unassigned | Task 1: Lock topic and payload contracts in tests | `systems/flux/flux/strategies/shared/observability/contracts.py`, `systems/flux/flux/strategies/shared/observability/serialization.py`, `systems/flux/flux/strategies/shared/observability/publisher.py`, `systems/flux/flux/strategies/shared/alerts.py`, `systems/flux/flux/strategies/shared/trades.py`, `tests/unit_tests/flux/strategies/shared/test_trades.py`, `tests/unit_tests/flux/test_architecture_boundaries.py` | `wave/pr3-observability-platform` | `.worktrees/strategy-platform-pr3` | none | not_run | Plan created |
| Task 3: Migrate all strategy, runner, and shared consumers, delete upward imports | not_started | unassigned | Task 2: Extract shared observability modules | `systems/flux/flux/strategies/makerv3/publisher.py`, `systems/flux/flux/strategies/makerv4/strategy.py`, `systems/flux/flux/strategies/shared/equities_arb/hedging.py`, `systems/flux/flux/strategies/shared/alerts.py`, `systems/flux/flux/runners/tokenmm/readiness.py`, `systems/flux/flux/runners/tokenmm/run_bridge.py`, `systems/flux/flux/runners/tokenmm/run_node.py`, `systems/flux/flux/runners/equities/run_bridge.py`, `systems/flux/flux/runners/equities/run_node.py`, `tests/unit_tests/flux/strategies/makerv4/test_publisher_contract.py`, `tests/unit_tests/flux/strategies/makerv4/test_observability_and_exports.py`, `tests/unit_tests/flux/runners/test_tokenmm_readiness.py`, `tests/unit_tests/examples/strategies/test_tokenmm_run_bridge.py`, `tests/unit_tests/examples/strategies/test_equities_run_bridge.py`, `tests/unit_tests/flux/api/test_payload_snapshots.py`, `tests/unit_tests/flux/test_architecture_boundaries.py` | `wave/pr3-observability-platform` | `.worktrees/strategy-platform-pr3` | none | not_run | Plan created |
| Task 4: Verify payload preservation and record rollback note | not_started | unassigned | Task 3: Migrate all strategy and shared consumers, delete upward imports | `systems/flux/docs/makerv3.md`, `docs/plans/2026-03-31-strategy-platform-hardening-wave-pr3-shared-observability-platform.md` | `wave/pr3-observability-platform` | `.worktrees/strategy-platform-pr3` | none | not_run | Plan created |

---

### Task 1: Lock topic and payload contracts in tests

**Files:**
- Modify: `tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py`
- Modify: `tests/unit_tests/flux/strategies/makerv4/test_observability_and_exports.py`
- Modify: `tests/unit_tests/flux/strategies/shared/test_trades.py`
- Modify: `tests/unit_tests/flux/strategies/shared/test_equities_arb_core.py`
- Modify: `tests/unit_tests/flux/api/test_payloads.py`
- Create: `tests/unit_tests/flux/api/test_payload_snapshots.py`
- Modify: `tests/unit_tests/flux/api/test_realtime_contract.py`
- Modify: `tests/unit_tests/flux/runners/test_tokenmm_readiness.py`
- Modify: `tests/unit_tests/examples/strategies/test_tokenmm_run_bridge.py`
- Modify: `tests/unit_tests/examples/strategies/test_equities_run_bridge.py`
- Modify: `tests/unit_tests/flux/test_architecture_boundaries.py`

**Dependencies:** `none`

**Write Scope:** `tests/unit_tests/flux/test_architecture_boundaries.py`, `tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py`, `tests/unit_tests/flux/strategies/makerv4/test_observability_and_exports.py`, `tests/unit_tests/flux/strategies/shared/test_trades.py`, `tests/unit_tests/flux/strategies/shared/test_equities_arb_core.py`, `tests/unit_tests/flux/api/test_payloads.py`, `tests/unit_tests/flux/api/test_payload_snapshots.py`, `tests/unit_tests/flux/api/test_realtime_contract.py`, `tests/unit_tests/flux/runners/test_tokenmm_readiness.py`, `tests/unit_tests/examples/strategies/test_tokenmm_run_bridge.py`, `tests/unit_tests/examples/strategies/test_equities_run_bridge.py`

**Verification Commands:**
- `python -c "import ibapi"`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/test_architecture_boundaries.py tests/unit_tests/flux/strategies/shared/test_trades.py tests/unit_tests/flux/strategies/shared/test_equities_arb_core.py tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py tests/unit_tests/flux/api/test_payloads.py tests/unit_tests/flux/api/test_payload_snapshots.py tests/unit_tests/flux/api/test_realtime_contract.py tests/unit_tests/flux/runners/test_tokenmm_readiness.py tests/unit_tests/examples/strategies/test_tokenmm_run_bridge.py tests/unit_tests/examples/strategies/test_equities_run_bridge.py -q`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/strategies/makerv4/test_observability_and_exports.py -q`

**Step 1: Write the failing tests**

Pin:

- topic names
- payload keys and semantics
- golden payload fixtures for representative alert/event/balances/order-intent/trade envelopes
- trade quantity contract
- strategy state/event export semantics
- readiness and bridge routing semantics for the frozen legacy topics

**Step 2: Run tests to verify they fail**

Run the shared/Makerv3/API slice in `default-unit` and the Makerv4 slice in `ibkr-unit`.

**Step 3: Commit**

```bash
git add tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py \
  tests/unit_tests/flux/strategies/makerv4/test_observability_and_exports.py \
  tests/unit_tests/flux/strategies/shared/test_trades.py \
  tests/unit_tests/flux/strategies/shared/test_equities_arb_core.py \
  tests/unit_tests/flux/api/test_payloads.py \
  tests/unit_tests/flux/api/test_payload_snapshots.py \
  tests/unit_tests/flux/api/test_realtime_contract.py \
  tests/unit_tests/flux/runners/test_tokenmm_readiness.py \
  tests/unit_tests/examples/strategies/test_tokenmm_run_bridge.py \
  tests/unit_tests/examples/strategies/test_equities_run_bridge.py \
  tests/unit_tests/flux/test_architecture_boundaries.py
git commit -m "test: lock observability platform contracts"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 2: Extract shared observability modules

**Files:**
- Create: `systems/flux/flux/strategies/shared/observability/contracts.py`
- Create: `systems/flux/flux/strategies/shared/observability/serialization.py`
- Create: `systems/flux/flux/strategies/shared/observability/publisher.py`
- Modify: `systems/flux/flux/strategies/shared/alerts.py`
- Modify: `systems/flux/flux/strategies/shared/trades.py`
- Modify: `tests/unit_tests/flux/strategies/shared/test_trades.py`
- Modify: `tests/unit_tests/flux/test_architecture_boundaries.py`

**Dependencies:** `Task 1: Lock topic and payload contracts in tests`

**Write Scope:** `systems/flux/flux/strategies/shared/observability/contracts.py`, `systems/flux/flux/strategies/shared/observability/serialization.py`, `systems/flux/flux/strategies/shared/observability/publisher.py`, `systems/flux/flux/strategies/shared/alerts.py`, `systems/flux/flux/strategies/shared/trades.py`, `tests/unit_tests/flux/strategies/shared/test_trades.py`, `tests/unit_tests/flux/test_architecture_boundaries.py`

**Verification Commands:**
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/test_architecture_boundaries.py tests/unit_tests/flux/strategies/shared/test_trades.py -q -k 'architecture or trade'`

**Step 1: Add the new shared helper modules**

Move generic:

- topic constants and frozen payload-schema helpers under `observability/contracts`
- decimal/json serialization
- generic publish helpers
- alert helper logic

into shared ownership.

**Step 2: Keep external contracts stable**

The helper modules may move, but topic names and payload field names must remain unchanged.

**Step 3: Commit**

```bash
git add systems/flux/flux/strategies/shared/observability/contracts.py \
  systems/flux/flux/strategies/shared/observability/serialization.py \
  systems/flux/flux/strategies/shared/observability/publisher.py \
  systems/flux/flux/strategies/shared/alerts.py \
  systems/flux/flux/strategies/shared/trades.py \
  tests/unit_tests/flux/strategies/shared/test_trades.py \
  tests/unit_tests/flux/test_architecture_boundaries.py
git commit -m "feat: extract shared observability helpers"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 3: Migrate all strategy, runner, and shared consumers, delete upward imports

**Files:**
- Modify: `systems/flux/flux/strategies/makerv3/publisher.py`
- Modify: `systems/flux/flux/strategies/makerv4/strategy.py`
- Modify: `systems/flux/flux/strategies/shared/equities_arb/hedging.py`
- Modify: `systems/flux/flux/strategies/shared/alerts.py`
- Modify: `systems/flux/flux/runners/tokenmm/readiness.py`
- Modify: `systems/flux/flux/runners/tokenmm/run_bridge.py`
- Modify: `systems/flux/flux/runners/tokenmm/run_node.py`
- Modify: `systems/flux/flux/runners/equities/run_bridge.py`
- Modify: `systems/flux/flux/runners/equities/run_node.py`
- Modify: `tests/unit_tests/flux/strategies/makerv4/test_publisher_contract.py`
- Modify: `tests/unit_tests/flux/strategies/makerv4/test_observability_and_exports.py`
- Modify: `tests/unit_tests/flux/runners/test_tokenmm_readiness.py`
- Modify: `tests/unit_tests/examples/strategies/test_tokenmm_run_bridge.py`
- Modify: `tests/unit_tests/examples/strategies/test_equities_run_bridge.py`
- Modify: `tests/unit_tests/flux/api/test_payload_snapshots.py`
- Modify: `tests/unit_tests/flux/test_architecture_boundaries.py`

**Dependencies:** `Task 2: Extract shared observability modules`

**Write Scope:** `systems/flux/flux/strategies/makerv3/publisher.py`, `systems/flux/flux/strategies/makerv4/strategy.py`, `systems/flux/flux/strategies/shared/equities_arb/hedging.py`, `systems/flux/flux/strategies/shared/alerts.py`, `systems/flux/flux/runners/tokenmm/readiness.py`, `systems/flux/flux/runners/tokenmm/run_bridge.py`, `systems/flux/flux/runners/tokenmm/run_node.py`, `systems/flux/flux/runners/equities/run_bridge.py`, `systems/flux/flux/runners/equities/run_node.py`, `tests/unit_tests/flux/strategies/makerv4/test_publisher_contract.py`, `tests/unit_tests/flux/strategies/makerv4/test_observability_and_exports.py`, `tests/unit_tests/flux/runners/test_tokenmm_readiness.py`, `tests/unit_tests/examples/strategies/test_tokenmm_run_bridge.py`, `tests/unit_tests/examples/strategies/test_equities_run_bridge.py`, `tests/unit_tests/flux/api/test_payload_snapshots.py`, `tests/unit_tests/flux/test_architecture_boundaries.py`

**Verification Commands:**
- `python -c "import ibapi"`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/strategies/makerv4/test_publisher_contract.py tests/unit_tests/flux/strategies/makerv4/test_observability_and_exports.py -q`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/runners/test_tokenmm_readiness.py tests/unit_tests/examples/strategies/test_tokenmm_run_bridge.py tests/unit_tests/examples/strategies/test_equities_run_bridge.py tests/unit_tests/flux/api/test_payload_snapshots.py tests/unit_tests/flux/test_architecture_boundaries.py -q`
- `rg -n "from flux\\.strategies\\.makerv3|import .*makerv3" systems/flux/flux/strategies/{makerv4,shared} systems/flux/flux/runners/{tokenmm,equities}`

**Step 1: Migrate Makerv4, runner, and shared-family consumers**

Point Makerv4, bridge/readiness consumers, and `shared/*` modules at the new shared observability helpers.

**Step 2: Slim Makerv3 publisher**

Keep only family-specific state-building logic in `makerv3/publisher.py`; shared low-level helpers should now import from `strategies/shared`.

**Step 3: Verify upward imports are gone across strategy and runner consumers**

The ripgrep command must show no Makerv3 publisher/constants imports in Makerv4, `shared/*`, or the runner bridge/readiness modules migrated in this PR.

**Step 4: Commit**

```bash
git add systems/flux/flux/strategies/makerv3/publisher.py \
  systems/flux/flux/strategies/makerv4/strategy.py \
  systems/flux/flux/strategies/shared/equities_arb/hedging.py \
  systems/flux/flux/strategies/shared/alerts.py \
  tests/unit_tests/flux/strategies/makerv4/test_publisher_contract.py \
  tests/unit_tests/flux/strategies/makerv4/test_observability_and_exports.py \
  tests/unit_tests/flux/api/test_payload_snapshots.py \
  tests/unit_tests/flux/test_architecture_boundaries.py
git commit -m "refactor: migrate consumers to shared observability platform"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 4: Verify payload preservation and record rollback note

**Files:**
- Modify: `systems/flux/docs/makerv3.md`
- Modify: `docs/plans/2026-03-31-strategy-platform-hardening-wave-pr3-shared-observability-platform.md`

**Dependencies:** `Task 3: Migrate all strategy and shared consumers, delete upward imports`

**Write Scope:** `systems/flux/docs/makerv3.md`, `docs/plans/2026-03-31-strategy-platform-hardening-wave-pr3-shared-observability-platform.md`

**Verification Commands:**
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/test_architecture_boundaries.py tests/unit_tests/flux/strategies/shared/test_trades.py tests/unit_tests/flux/strategies/shared/test_equities_arb_core.py tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py tests/unit_tests/flux/api/test_payloads.py tests/unit_tests/flux/api/test_payload_snapshots.py tests/unit_tests/flux/api/test_realtime_contract.py tests/unit_tests/flux/runners/test_tokenmm_readiness.py tests/unit_tests/examples/strategies/test_tokenmm_run_bridge.py tests/unit_tests/examples/strategies/test_equities_run_bridge.py -q`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest tests/unit_tests/flux/strategies/makerv4/test_observability_and_exports.py tests/unit_tests/flux/strategies/makerv4/test_publisher_contract.py -q`
- `git diff --check`

**Step 1: Run the combined bundle**

The shared helper tests, strategy export tests, runner bridge/readiness tests, and API payload tests must pass together.

**Step 2: Update docs**

State explicitly that helper ownership changed while topic names and payload keys remained frozen.

**Step 3: Commit**

```bash
git add systems/flux/docs/makerv3.md \
  docs/plans/2026-03-31-strategy-platform-hardening-wave-pr3-shared-observability-platform.md
git commit -m "docs: record pr3 verification and rollback"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

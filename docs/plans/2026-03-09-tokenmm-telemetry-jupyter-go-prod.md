# TokenMM Telemetry + Jupyter Go-Prod Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task in the current session.
> **Progress:** The Progress Tracker in this document is the execution source of truth and must be updated on every state change.

**Goal:** Bring merged `main` to parity with the currently running TokenMM live stack while adding local telemetry persistence, Postgres shipping hooks, and a localhost-only JupyterLab surface with an example trade-data notebook.

**Architecture:** Port the telemetry persistence and shipper surfaces from the reviewed branch into current `main`, but keep the existing 7-strategy TokenMM topology that is already running on the host. Wire persistence off the trading hot path through local SQLite actors and a separate shipper, then add a separate localhost-only JupyterLab service that reads the same local telemetry files for research and operations.

**Tech Stack:** Python 3.12/3.13, `uv`, `pytest`, SQLite, PostgreSQL shipper, systemd `flux@.service`, TokenMM Flux runners, JupyterLab.

## Progress Tracker

**Source of truth:** Update this table whenever task state changes. Do not rely on memory, chat history, or TodoWrite alone.

| Task | Status | Owner | Notes / Last Update |
| --- | --- | --- | --- |
| Overall | completed | main | Tasks 1-5 implemented; final targeted verification passed (`195 passed in 2.21s`) and notebook JSON validated |
| Task 1: Port Telemetry Persistence Foundations | completed | main | Spec + quality approved; `python3.12 -m pytest ...` => `78 passed in 3.05s` |
| Task 2: Wire Flux Persistence + TokenMM Runners | completed | main | Spec + quality approved; `python3.12 -m pytest ...` => `83 passed in 0.74s` after portfolio snapshot field persistence fix |
| Task 3: Restore Prod Topology Parity + Fix Runtime Gaps | completed | main | Spec approved; `python3.12 -m pytest tests/unit_tests/examples/strategies/test_tokenmm_stack_contract.py tests/unit_tests/flux/pulse/test_api.py tests/unit_tests/flux/strategies/makerv3/test_order_intent_exports.py tests/unit_tests/flux/strategies/makerv3/test_order_safety.py -q` => `64 passed in 0.89s` |
| Task 4: Add JupyterLab Service + Example Notebook | completed | main | Added notebook dependency group, localhost-only env template, research docs, and `tokenmm_trade_data.ipynb`; `python3.12 -m pytest tests/unit_tests/examples/strategies/test_tokenmm_stack_contract.py -q` => `39 passed in 0.29s` |
| Task 5: Update Runbooks + Verify Cutover Commands | completed | main | Added telemetry/Jupyter cutover docs and RDS runbook; final targeted verification `195 passed in 2.21s`, `python3.12 -m json.tool research/tokenmm/notebooks/tokenmm_trade_data.ipynb >/dev/null` |

---

### Task 1: Port Telemetry Persistence Foundations

**Files:**
- Create: `nautilus_trader/persistence/_action_intent.py`
- Create: `nautilus_trader/persistence/_async_sqlite.py`
- Create: `nautilus_trader/persistence/_execution_timing.py`
- Create: `nautilus_trader/persistence/fills/__init__.py`
- Create: `nautilus_trader/persistence/fills/actor.py`
- Create: `nautilus_trader/persistence/fills/config.py`
- Create: `nautilus_trader/persistence/fills/schema.py`
- Create: `nautilus_trader/persistence/fills/sqlite.py`
- Create: `nautilus_trader/persistence/orders/__init__.py`
- Create: `nautilus_trader/persistence/orders/actor.py`
- Create: `nautilus_trader/persistence/orders/config.py`
- Create: `nautilus_trader/persistence/orders/schema.py`
- Create: `nautilus_trader/persistence/orders/sqlite.py`
- Create: `nautilus_trader/persistence/shipper/__init__.py`
- Create: `nautilus_trader/persistence/shipper/config.py`
- Create: `nautilus_trader/persistence/shipper/postgres.py`
- Create: `nautilus_trader/persistence/shipper/run.py`
- Create: `nautilus_trader/persistence/shipper/service.py`
- Modify: `nautilus_trader/persistence/__init__.py`
- Modify: `pyproject.toml`
- Modify: `uv.lock`
- Test: `tests/unit_tests/persistence/test_action_intent.py`
- Test: `tests/unit_tests/persistence/test_execution_fill_persistence_actor.py`
- Test: `tests/unit_tests/persistence/test_execution_fill_sqlite.py`
- Test: `tests/unit_tests/persistence/test_execution_timing.py`
- Test: `tests/unit_tests/persistence/test_order_action_persistence_actor.py`
- Test: `tests/unit_tests/persistence/test_order_action_sqlite.py`
- Test: `tests/unit_tests/persistence/test_telemetry_shipper.py`

**Step 1: Write the failing tests**

Use the PR-reviewed persistence tests as the spec source and add them before porting implementation files.

**Step 2: Run tests to verify they fail**

Run: `uv run --group test python -m pytest tests/unit_tests/persistence/test_action_intent.py tests/unit_tests/persistence/test_execution_fill_persistence_actor.py tests/unit_tests/persistence/test_execution_fill_sqlite.py tests/unit_tests/persistence/test_execution_timing.py tests/unit_tests/persistence/test_order_action_persistence_actor.py tests/unit_tests/persistence/test_order_action_sqlite.py tests/unit_tests/persistence/test_telemetry_shipper.py -q`

Expected: FAIL on missing persistence modules/classes.

**Step 3: Port the minimal implementation**

Bring in the persistence modules from the reviewed branch, add the base `psycopg[binary]` dependency required by the shipper, and adapt imports so they work on current `main`.

**Step 4: Run tests to verify they pass**

Run the same command from Step 2.

Expected: PASS.

**Step 5: Commit**

```bash
git add nautilus_trader/persistence tests/unit_tests/persistence
git commit -m "feat: add tokenmm telemetry persistence foundations"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 2: Wire Flux Persistence + TokenMM Runners

**Files:**
- Create: `systems/flux/flux/persistence/__init__.py`
- Create: `systems/flux/flux/persistence/balance_snapshots/__init__.py`
- Create: `systems/flux/flux/persistence/balance_snapshots/actor.py`
- Create: `systems/flux/flux/persistence/balance_snapshots/config.py`
- Create: `systems/flux/flux/persistence/balance_snapshots/normalize.py`
- Create: `systems/flux/flux/persistence/balance_snapshots/schema.py`
- Create: `systems/flux/flux/persistence/balance_snapshots/sqlite.py`
- Create: `systems/flux/flux/persistence/portfolio_inventory_snapshots/__init__.py`
- Create: `systems/flux/flux/persistence/portfolio_inventory_snapshots/schema.py`
- Create: `systems/flux/flux/persistence/portfolio_inventory_snapshots/sqlite.py`
- Create: `systems/flux/flux/persistence/quote_cycles/__init__.py`
- Create: `systems/flux/flux/persistence/quote_cycles/actor.py`
- Create: `systems/flux/flux/persistence/quote_cycles/config.py`
- Create: `systems/flux/flux/persistence/quote_cycles/schema.py`
- Create: `systems/flux/flux/persistence/quote_cycles/sqlite.py`
- Create: `nautilus_trader/flux/__init__.py`
- Modify: `systems/flux/flux/runners/tokenmm/run_node.py`
- Modify: `systems/flux/flux/runners/tokenmm/run_portfolio.py`
- Modify: `systems/flux/flux/runners/tokenmm/run_bridge.py`
- Modify: `systems/flux/flux/runners/tokenmm/run_api.py`
- Modify: `systems/flux/flux/runners/tokenmm/redis_runtime.py`
- Test: `tests/unit_tests/persistence/test_flux_balance_snapshot_actor.py`
- Test: `tests/unit_tests/persistence/test_flux_balance_snapshot_sqlite.py`
- Test: `tests/unit_tests/persistence/test_portfolio_inventory_snapshot_sqlite.py`
- Test: `tests/unit_tests/persistence/test_quote_cycle_persistence_actor.py`
- Test: `tests/unit_tests/examples/strategies/test_tokenmm_run_node.py`
- Test: `tests/unit_tests/examples/strategies/test_tokenmm_run_bridge.py`
- Test: `tests/unit_tests/examples/strategies/test_tokenmm_run_portfolio.py`
- Test: `tests/unit_tests/examples/strategies/test_tokenmm_run_api.py`

**Step 1: Write/port the failing runner and Flux persistence tests**

Add the reviewed Flux persistence tests and runner contract tests before porting the implementation files.

**Step 2: Run tests to verify they fail**

Run: `uv run --group test python -m pytest tests/unit_tests/persistence/test_flux_balance_snapshot_actor.py tests/unit_tests/persistence/test_flux_balance_snapshot_sqlite.py tests/unit_tests/persistence/test_portfolio_inventory_snapshot_sqlite.py tests/unit_tests/persistence/test_quote_cycle_persistence_actor.py tests/unit_tests/examples/strategies/test_tokenmm_run_node.py tests/unit_tests/examples/strategies/test_tokenmm_run_bridge.py tests/unit_tests/examples/strategies/test_tokenmm_run_portfolio.py tests/unit_tests/examples/strategies/test_tokenmm_run_api.py -q`

Expected: FAIL on missing Flux persistence modules and missing telemetry runner wiring.

**Step 3: Port the minimal implementation**

Add the Flux persistence modules, compatibility shim, and runner wiring needed to persist local telemetry and expose shipper config fields.

**Step 4: Run tests to verify they pass**

Run the same command from Step 2.

Expected: PASS.

**Step 5: Commit**

```bash
git add systems/flux/flux/persistence systems/flux/flux/runners/tokenmm nautilus_trader/flux tests/unit_tests/examples/strategies tests/unit_tests/persistence
git commit -m "feat: wire tokenmm telemetry persistence into flux runners"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 3: Restore Prod Topology Parity + Fix Runtime Gaps

**Files:**
- Modify: `deploy/tokenmm/tokenmm.live.toml`
- Modify: `deploy/tokenmm/strategies/README.md`
- Modify: `deploy/tokenmm/strategies/plumeusdt_binance_perp_makerv3.toml`
- Modify: `deploy/tokenmm/strategies/plumeusdt_binance_spot_makerv3.toml`
- Modify: `deploy/tokenmm/strategies/plumeusdt_bitget_perp_makerv3.toml`
- Modify: `deploy/tokenmm/strategies/plumeusdt_bitget_spot_makerv3.toml`
- Modify: `deploy/tokenmm/strategies/plumeusdt_bybit_perp_makerv3.toml`
- Modify: `deploy/tokenmm/strategies/plumeusdt_bybit_spot_makerv3.toml`
- Modify: `deploy/tokenmm/strategies/plumeusdt_okx_perp_makerv3.toml`
- Modify: `deploy/tokenmm/systemd/common.env.example`
- Modify: `deploy/tokenmm/systemd/flux-tokenmm.target`
- Modify: `deploy/tokenmm/systemd/flux-pulse.sudoers`
- Modify: `deploy/tokenmm/README.md`
- Create: `deploy/tokenmm/TELEMETRY_RDS_RUNBOOK.md`
- Modify: `ops/scripts/deploy/install_tokenmm_systemd.sh`
- Modify: `ops/scripts/deploy/tokenmm_stack.sh`
- Modify: `systems/flux/flux/strategies/makerv3/strategy.py`
- Modify: `systems/flux/flux/strategies/makerv3/quote_engine.py`
- Modify: `systems/flux/flux/strategies/makerv3/failures.py`
- Test: `tests/unit_tests/examples/strategies/test_tokenmm_stack_contract.py`
- Test: `tests/unit_tests/flux/pulse/test_api.py`
- Test: `tests/unit_tests/flux/strategies/makerv3/test_order_intent_exports.py`
- Test: `tests/unit_tests/flux/strategies/makerv3/test_order_safety.py`

**Step 1: Write the failing tests**

Add or update tests to lock:
- current 7-strategy prod parity including Bitget
- installable `flux-tokenmm.target`
- preserved `decision_context_json`
- no enrichment loss when IDs diverge
- safer API bind defaults / docs alignment where applicable

**Step 2: Run tests to verify they fail**

Run: `uv run --group test python -m pytest tests/unit_tests/examples/strategies/test_tokenmm_stack_contract.py tests/unit_tests/flux/pulse/test_api.py tests/unit_tests/flux/strategies/makerv3/test_order_intent_exports.py tests/unit_tests/flux/strategies/makerv3/test_order_safety.py -q`

Expected: FAIL on one or more of the locked parity/runtime gaps.

**Step 3: Write minimal implementation**

Patch deploy/runtime surfaces so `main` can replace the currently running host config without losing active strategies or telemetry context.

**Step 4: Run tests to verify they pass**

Run the same command from Step 2.

Expected: PASS.

**Step 5: Commit**

```bash
git add deploy/tokenmm ops/scripts/deploy systems/flux/flux/strategies/makerv3 tests/unit_tests/examples/strategies/test_tokenmm_stack_contract.py tests/unit_tests/flux/pulse/test_api.py tests/unit_tests/flux/strategies/makerv3
git commit -m "fix: align tokenmm prod topology and telemetry runtime contracts"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 4: Add JupyterLab Service + Example Notebook

**Files:**
- Modify: `pyproject.toml`
- Modify: `uv.lock`
- Create: `deploy/tokenmm/systemd/tokenmm-jupyter.env.example`
- Modify: `deploy/tokenmm/README.md`
- Modify: `deploy/tokenmm/systemd/common.env.example`
- Modify: `ops/scripts/deploy/install_tokenmm_systemd.sh`
- Create: `research/tokenmm/README.md`
- Create: `research/tokenmm/notebooks/tokenmm_trade_data.ipynb`
- Test: `tests/unit_tests/examples/strategies/test_tokenmm_stack_contract.py`

**Step 1: Write the failing test**

Extend deploy contract coverage so the repo must ship a localhost-only Jupyter service template and notebook docs.

**Step 2: Run test to verify it fails**

Run: `uv run --group test python -m pytest tests/unit_tests/examples/strategies/test_tokenmm_stack_contract.py -q`

Expected: FAIL on missing Jupyter assets/contracts.

**Step 3: Write minimal implementation**

Add a notebook dependency group, a localhost-only Jupyter env template, install-script support, and a notebook that loads local SQLite `execution_fill`, `order_action`, and `quote_cycle` data with optional Postgres notes.

**Step 4: Run tests to verify it passes**

Run: `uv run --group test python -m pytest tests/unit_tests/examples/strategies/test_tokenmm_stack_contract.py -q`

Expected: PASS.

**Step 5: Commit**

```bash
git add pyproject.toml uv.lock deploy/tokenmm research/tokenmm tests/unit_tests/examples/strategies/test_tokenmm_stack_contract.py
git commit -m "feat: add tokenmm jupyter notebook service and trade-data notebook"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 5: Update Runbooks + Verify Cutover Commands

**Files:**
- Modify: `docs/flux/api.md`
- Modify: `docs/fluxboard/tokenmm_runbook.md`
- Modify: `deploy/tokenmm/README.md`
- Modify: `deploy/tokenmm/TELEMETRY_RDS_RUNBOOK.md`
- Create: `docs/plans/2026-03-09-tokenmm-telemetry-jupyter-go-prod-design.md`

**Step 1: Write the failing doc/contract expectations**

Lock the expected cutover instructions and security notes in the existing docs/tests before changing prose.

**Step 2: Run verification-focused tests**

Run: `uv run --group test python -m pytest tests/unit_tests/examples/strategies/test_tokenmm_stack_contract.py tests/unit_tests/flux/pulse/test_api.py -q`

Expected: FAIL if docs/contracts are still stale.

**Step 3: Write minimal docs and runbook updates**

Document:
- current live-to-main cutover path
- telemetry shipper bootstrap
- localhost-only Jupyter start path
- exact restart order for the already-running services
- SQL/SQLite verification commands

**Step 4: Run final targeted verification**

Run:
- `uv run --group test python -m pytest tests/unit_tests/persistence/test_action_intent.py tests/unit_tests/persistence/test_execution_fill_persistence_actor.py tests/unit_tests/persistence/test_order_action_persistence_actor.py tests/unit_tests/persistence/test_telemetry_shipper.py tests/unit_tests/examples/strategies/test_tokenmm_run_node.py tests/unit_tests/examples/strategies/test_tokenmm_run_bridge.py tests/unit_tests/examples/strategies/test_tokenmm_run_portfolio.py tests/unit_tests/examples/strategies/test_tokenmm_run_api.py tests/unit_tests/examples/strategies/test_tokenmm_stack_contract.py tests/unit_tests/flux/pulse/test_api.py tests/unit_tests/flux/strategies/makerv3/test_order_intent_exports.py tests/unit_tests/flux/strategies/makerv3/test_order_safety.py -q`
- `python3 -m json.tool research/tokenmm/notebooks/tokenmm_trade_data.ipynb >/dev/null`

Expected: PASS.

**Step 5: Commit**

```bash
git add docs/flux/api.md docs/fluxboard/tokenmm_runbook.md deploy/tokenmm/README.md deploy/tokenmm/TELEMETRY_RDS_RUNBOOK.md docs/plans/2026-03-09-tokenmm-telemetry-jupyter-go-prod-design.md
git commit -m "docs: add tokenmm telemetry and jupyter cutover runbook"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

# Equities Shared Symbol-Node Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.
> **Progress:** The Progress Tracker in this document is the execution source of truth and must be updated on every state change.

**Goal:** Collapse the live equities runtime from one node per strategy to one node per symbol plus maker venue, while preserving the existing `38` external strategy IDs and the shared `/equities` control plane.

**Architecture:** Keep the current split maker/taker strategy families and strategy TOMLs, but derive grouped node definitions from `strategy_contracts` plus enrolled strategy configs. The equities node runner will host one or two strategies per `TradingNode`, the API/Pulse layer will map external strategy IDs onto grouped node jobs, and the installer will render one node service per derived node group instead of one per strategy.

**Tech Stack:** Python, Nautilus live runners, systemd/bash deploy scripts, pytest, immutable release-root deploys, Pulse/Fluxboard.

**Context Docs:**
- Design: `docs/plans/2026-03-27-equities-shared-symbol-node-design.md`
- PRD: `none`
- Relevant specs/runbooks: `docs/plans/2026-03-17-equities-makerv4-split-design.md`, `deploy/equities/README.md`, `deploy/equities/strategies/README.md`, `fluxboard/docs/equities_contract.md`, `systems/flux/docs/api.md`, `tests/unit_tests/examples/strategies/test_equities_run_node.py`, `tests/unit_tests/examples/strategies/test_equities_run_api.py`, `tests/unit_tests/ops/deploy/test_install_equities_systemd.py`

**Decision Summary:**
- Keep the external `38` strategy IDs and all existing `equities` API/Fluxboard routes unchanged.
- Change only the runtime/deploy topology: `19` node services, each hosting one or two strategies.
- Keep per-strategy TOMLs for this wave; do not invent a new node config format.
- Use a canonical Python grouping helper so runtime and installer share the same grouping rules.
- Change node-scoped runtime identity from external strategy ID to node-group ID.
- Treat `run_api.py`, Pulse, sudoers, and checked-in target assets as part of the compatibility surface, not follow-up cleanup.
- Require an atomic full-stack cutover and rollback; no mixed old/new node registries.
- Do not rely on a second IBKR gateway for this stabilization wave.

## Progress Tracker

**Source of truth:** Update this table whenever task state changes. Do not rely on memory, chat history, or TodoWrite alone.

| Task | Status | Owner | Notes / Last Update |
| --- | --- | --- | --- |
| Overall | in_progress | controller | Execution started via subagent-driven-development |
| Task 1: Add Canonical Equities Node-Grouping Contract | completed | controller | Completed via commits 4237f1ee07 and ec77a633d6; spec and quality reviews passed |
| Task 2: Refactor The Runner For Shared-Node Ownership Semantics | completed | controller | Completed via 65edb3c33c, 4b96a749ef, d2f56e43d4, a9553f045e, 0494bdd344, and 1ec023cc30; spec and quality reviews passed |
| Task 3: Preserve API, Pulse, And Strategy-Level External Contracts | completed | controller | Completed via commits 1ce27910b3 and bf194746ab; spec and quality reviews passed |
| Task 4: Collapse Installer, Target, And Sudoers To Grouped Node Jobs | completed | controller | Committed as feb8559d43; targeted and broad Task 4 verification green |
| Task 5: Controlled Prod Cutover And Atomic Rollback Plan | in_progress | controller | `2026-03-27 19:28-21:49 UTC`: targeted WIP verification is green in the project venv (`equities_maker`, `equities_taker`, payload slices) plus the equities runner/contract suites updated for a single shared IBKR gateway (`test_equities_stack_contract.py`, `test_equities_run_portfolio.py`, `test_equities_run_node.py`). Immutable releases `~/releases/prod/equities/releases/20260327T192840Z-quote-liveness-recovery` and `~/releases/prod/equities/releases/20260327T213644Z-single-ibg-cleanup` were cut from the worktree; the installer now exports `PYTHONDONTWRITEBYTECODE=1`, and the live profile no longer relies on `4002` or `ibg_fallback_ports`. The cutover re-rendered `/etc/flux/equities*.env` against the new release-local `.venv`, retired `nautilus-ib-gateway-live-4002` (`restart=no`, stopped), re-authenticated the remaining `nautilus-ib-gateway-live` gateway on `127.0.0.1:4001`, and restarted the equities stack plus `chainsaw@md-ibkr-publisher.service`. Current live state: public `/equities` and loopback/public signals APIs are serving `200`, IBKR auth/projections are healthy, direct/public signal rows are moving again, and readiness improved from `healthy_strategy_count=0` to `34` with `stale_signal_leg_count=3`. Remaining blocker: three maker-market-data legs (`EWY` on Binance Perp, `NVDA` + `ORCL` on Hyperliquid) still age out even though the MakerV4 stale-quote timer is actively issuing repeated unsubscribe/resubscribe commands, and `balances` remains degraded under `portfolio_snapshot_v2`; deeper venue/client lifecycle tracing is still required if those legs do not self-recover. |

---

### Task 1: Add Canonical Equities Node-Grouping Contract

**Files:**
- Create: `systems/flux/flux/runners/equities/node_groups.py`
- Create: `tests/unit_tests/examples/strategies/test_equities_node_groups.py`
- Modify: `tests/unit_tests/examples/strategies/test_equities_stack_contract.py`
- Modify: `deploy/equities/README.md`
- Modify: `deploy/equities/strategies/README.md`
- Modify: `fluxboard/docs/equities_contract.md`

**Step 1: Write the failing grouping tests**

Add focused tests that prove the current live basket groups into `19` stable node ids:

- `aapl_tradexyz_maker` + `aapl_tradexyz_taker` -> `aapl_tradexyz`
- `amzn_binance_perp_maker` + `amzn_binance_perp_taker` -> `amzn_binance_perp`
- total grouped-node count for the checked-in prod basket is `19`
- external enrolled strategy count remains `38`
- every external strategy id still resolves to exactly one grouped node id
- grouped-node docs are reflected in both deploy READMEs, not just the top-level one
- Fluxboard equities contract docs stop claiming one node process per strategy

Also add contract assertions in `test_equities_stack_contract.py` that:

- the checked-in deploy docs describe grouped nodes, not one node process per strategy
- the checked-in strategy docs explicitly say per-strategy TOMLs remain the source of strategy-local config, but service registry is grouped-node based
- the checked-in Fluxboard equities contract does not claim one node process per strategy

**Step 2: Run the tests to confirm they fail**

Run: `pytest -q tests/unit_tests/examples/strategies/test_equities_node_groups.py tests/unit_tests/examples/strategies/test_equities_stack_contract.py -k 'node_group or grouped_node'`

Expected: FAIL because there is no canonical grouping helper and the docs still describe one node per strategy.

**Step 3: Implement the minimal grouping helper**

Add a canonical helper module that:

- reads `strategy_contracts`
- inspects enrolled strategy TOMLs
- derives a stable node-group id from each split strategy pair
- returns group metadata including member strategy ids and config paths

Keep the output small and deterministic so both runtime code and installer code can consume it directly.

**Step 4: Update docs to reflect the new contract**

Change `deploy/equities/README.md` so it describes:

- `38` strategy IDs
- `19` node services
- one grouped node per symbol plus maker venue

Change `deploy/equities/strategies/README.md` so it describes:

- one TOML file per strategy, not one service per TOML
- grouped node service naming and strategy membership
- strategy-local files staying the source of runtime defaults and external strategy identity

Update `fluxboard/docs/equities_contract.md` so it reflects:

- grouped nodes are an internal deploy detail
- `/equities` still exposes the existing strategy-level operator surface
- realtime behavior remains part of the external contract

**Step 5: Re-run the tests**

Run: `pytest -q tests/unit_tests/examples/strategies/test_equities_node_groups.py tests/unit_tests/examples/strategies/test_equities_stack_contract.py -k 'node_group or grouped_node'`

Expected: PASS

**Step 6: Commit**

```bash
git add systems/flux/flux/runners/equities/node_groups.py \
  tests/unit_tests/examples/strategies/test_equities_node_groups.py \
  tests/unit_tests/examples/strategies/test_equities_stack_contract.py \
  deploy/equities/README.md \
  deploy/equities/strategies/README.md \
  fluxboard/docs/equities_contract.md
git commit -m "feat: add equities grouped-node contract"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 2: Refactor The Runner For Shared-Node Ownership Semantics

**Files:**
- Modify: `systems/flux/flux/runners/equities/run_node.py`
- Modify: `tests/unit_tests/examples/strategies/test_equities_run_node.py`
- Modify: `tests/unit_tests/examples/strategies/test_runner_external_order_claims.py`

**Step 1: Write the failing runner tests**

Add tests that prove one grouped node can:

- accept multiple strategy config paths
- build one shared `TradingNodeConfig`
- attach both strategy instances to one `TradingNode`
- keep each strategy’s `external_strategy_id`
- switch the node-scoped stream prefix from external strategy id to node-group id
- keep runtime params attachment strategy-scoped
- keep portfolio inventory component publication strategy-scoped
- keep projection/reference-balance feed attachment strategy-scoped where required

Add at least one explicit test for a tradexyz pair and one for a binance-perp pair.

Add ownership tests that prove:

- same-node maker/taker siblings do not collapse order attribution to the node-group id
- duplicate external-order claims are still rejected correctly
- stray external orders remain attributable to the correct sibling strategy or remain unclaimed

**Step 2: Run the runner tests to confirm they fail**

Run: `pytest -q tests/unit_tests/examples/strategies/test_equities_run_node.py tests/unit_tests/examples/strategies/test_runner_external_order_claims.py -k 'grouped or multi_strategy or node_group'`

Expected: FAIL because the current runner builds exactly one strategy per node and keys message-bus identity off the external strategy id.

**Step 3: Implement grouped-node runner support**

Update `run_node.py` so it can:

- resolve one node group instead of exactly one strategy
- load multiple strategy configs from the same node group
- build one shared venue/client set
- instantiate one strategy object per member strategy
- attach all member strategies to the node before `node.build()`
- make node-scoped identity explicit instead of inferring it from the external strategy id
- preserve strategy-local runtime params, portfolio-inventory, and balance-projection hooks

Important implementation constraints:

- strategy payloads must still emit the original external strategy IDs
- execution/reconciliation scope stays node-level
- no strategy logic changes in maker/taker behavior
- no silent fallback to single-strategy semantics when more than one grouped member is present

**Step 4: Re-run the runner tests**

Run: `pytest -q tests/unit_tests/examples/strategies/test_equities_run_node.py tests/unit_tests/examples/strategies/test_runner_external_order_claims.py -k 'grouped or multi_strategy or node_group'`

Expected: PASS

**Step 5: Run the broader equities runner suite**

Run: `pytest -q tests/unit_tests/examples/strategies/test_equities_run_node.py`

Expected: PASS

**Step 6: Commit**

```bash
git add systems/flux/flux/runners/equities/run_node.py \
  tests/unit_tests/examples/strategies/test_equities_run_node.py \
  tests/unit_tests/examples/strategies/test_runner_external_order_claims.py
git commit -m "feat: run grouped equities strategies on one node"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 3: Preserve API, Pulse, And Strategy-Level External Contracts

**Files:**
- Modify: `systems/flux/flux/runners/equities/run_api.py`
- Modify: `tests/unit_tests/examples/strategies/test_equities_run_api.py`
- Modify: `tests/unit_tests/flux/api/test_equities_profile_contract.py`
- Modify: `tests/unit_tests/examples/strategies/test_equities_stack_contract.py`
- Modify: `fluxboard/docs/equities_contract.md`

**Step 1: Write the failing API/Pulse compatibility tests**

Add tests that prove:

- strategy running-state resolves external strategy ids through grouped Pulse job ids
- pulse-backed alerts resolve external strategy ids through grouped Pulse job ids
- `signals`, `params`, and `param-schema` stay keyed by external strategy id
- `balances` keeps the existing profile-level payload semantics and readiness metadata
- `trades` / `alerts` keep external strategy attribution when rows are present
- representative selector queries still work for both a tradexyz pair and a binance-perp pair
- profile contract tests explicitly guard against node-group ids leaking into external API payloads

**Step 2: Run the compatibility tests to confirm they fail**

Run: `pytest -q tests/unit_tests/examples/strategies/test_equities_run_api.py tests/unit_tests/flux/api/test_equities_profile_contract.py -k 'grouped or node_group or pulse or strategy_id'`

Expected: FAIL because `run_api.py` still assumes `equities-node-<strategy_id>` and the current contract tests do not cover grouped-node mapping.

**Step 3: Implement the grouped-node API/Pulse mapping**

Update `run_api.py` so it:

- resolves external strategy ids to grouped node job ids for running-state and pulse-backed alerts
- preserves external strategy ids in API rows and selector filters
- never leaks grouped node ids into API payloads
- continues to treat the grouped node topology as an internal operator concern

Also update the contract tests so grouped-node behavior is pinned in the checked-in equities API surface.
Update `fluxboard/docs/equities_contract.md` if needed so the checked-in public contract matches the grouped-node implementation.

**Step 4: Re-run the compatibility tests**

Run: `pytest -q tests/unit_tests/examples/strategies/test_equities_run_api.py tests/unit_tests/flux/api/test_equities_profile_contract.py -k 'grouped or node_group or pulse or strategy_id'`

Expected: PASS

**Step 5: Re-run the broader API suites**

Run: `pytest -q tests/unit_tests/examples/strategies/test_equities_run_api.py tests/unit_tests/flux/api/test_equities_profile_contract.py`

Expected: PASS

**Step 6: Commit**

```bash
git add systems/flux/flux/runners/equities/run_api.py \
  tests/unit_tests/examples/strategies/test_equities_run_api.py \
  tests/unit_tests/flux/api/test_equities_profile_contract.py \
  tests/unit_tests/examples/strategies/test_equities_stack_contract.py \
  fluxboard/docs/equities_contract.md
git commit -m "feat: preserve equities API contract on grouped nodes"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 4: Collapse Installer, Target, And Sudoers To Grouped Node Jobs

**Files:**
- Modify: `ops/scripts/deploy/install_equities_systemd.sh`
- Create: `ops/scripts/deploy/list_equities_node_groups.py`
- Modify: `tests/unit_tests/ops/deploy/test_install_equities_systemd.py`
- Modify: `tests/unit_tests/ops/deploy/test_rebuild_flux_pulse_sudoers.py`
- Modify: `tests/unit_tests/examples/strategies/test_equities_stack_contract.py`
- Modify: `deploy/equities/README.md`
- Modify: `deploy/equities/strategies/README.md`
- Modify: `fluxboard/docs/equities_contract.md`
- Modify: `deploy/equities/systemd/flux-equities.target`
- Modify: `deploy/equities/systemd/flux-pulse.sudoers`

**Step 1: Write the failing installer and operator-surface tests**

Add deploy-script and checked-in-asset tests that prove:

- the installer renders `equities-node-aapl_tradexyz.env`, not separate maker/taker node envs
- grouped node commands include both strategy config paths
- `flux-equities.target` wants `19` grouped node services, not `38`
- `flux-pulse.sudoers` is regenerated for grouped node services only
- stale per-strategy node envs are deleted on rerender
- the checked-in service registry, installer docs, and strategy docs all agree on grouped node names

**Step 2: Run the installer tests to confirm they fail**

Run: `pytest -q tests/unit_tests/ops/deploy/test_install_equities_systemd.py tests/unit_tests/ops/deploy/test_rebuild_flux_pulse_sudoers.py tests/unit_tests/examples/strategies/test_equities_stack_contract.py -k 'grouped or node_group or target or sudoers'`

Expected: FAIL because the installer still discovers strategies one file at a time and the checked-in target/sudoers assets still assume one service per strategy id.

**Step 3: Implement the shared installer discovery path**

Add a small Python helper script that emits grouped node metadata from the canonical grouping module. Update the bash installer and checked-in assets to:

- call the helper
- render one env file per node group
- include both strategy configs in the node command
- enroll grouped node service ids in the target
- rebuild grouped-node sudoers entries
- remove old `equities-node-<strategy_id>.env` files

Keep the existing API, portfolio, and bridge env generation unchanged.

**Step 4: Re-run the installer tests**

Run: `pytest -q tests/unit_tests/ops/deploy/test_install_equities_systemd.py tests/unit_tests/ops/deploy/test_rebuild_flux_pulse_sudoers.py tests/unit_tests/examples/strategies/test_equities_stack_contract.py -k 'grouped or node_group or target or sudoers'`

Expected: PASS

**Step 5: Re-run the broader deploy tests**

Run: `pytest -q tests/unit_tests/ops/deploy/test_install_equities_systemd.py tests/unit_tests/ops/deploy/test_rebuild_flux_pulse_sudoers.py tests/unit_tests/examples/strategies/test_equities_stack_contract.py`

Expected: PASS

**Step 6: Commit**

```bash
git add ops/scripts/deploy/install_equities_systemd.sh \
  ops/scripts/deploy/list_equities_node_groups.py \
  tests/unit_tests/ops/deploy/test_install_equities_systemd.py \
  tests/unit_tests/ops/deploy/test_rebuild_flux_pulse_sudoers.py \
  tests/unit_tests/examples/strategies/test_equities_stack_contract.py \
  deploy/equities/README.md \
  deploy/equities/strategies/README.md \
  fluxboard/docs/equities_contract.md \
  deploy/equities/systemd/flux-equities.target \
  deploy/equities/systemd/flux-pulse.sudoers
git commit -m "feat: render grouped equities operator surfaces"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 5: Controlled Prod Cutover And Atomic Rollback Plan

**Files:**
- Create: `docs/runbooks/equities-shared-node-cutover.md`
- Modify: `deploy/equities/README.md`
- Optional notes/logs only: `docs/reviews/`

**Step 1: Capture the live baseline**

Before changing anything on the host, capture the live baseline:

- current `~/releases/prod/equities/current`
- current `/etc/flux/equities*.env`
- current `flux-equities.target`
- current `/etc/sudoers.d/flux-pulse`
- current live node unit list and pulse-visible job list

The runbook must make this snapshot mandatory so rollback is exact, not memory-based.

**Step 2: Cut a fresh immutable release root**

Run:

```bash
bash ops/scripts/deploy/create_release_root.sh
```

Expected: a new release root under `~/releases/prod/equities/releases/<timestamp>-shared-nodes`

**Step 3: Build the release-local environment**

Run:

```bash
cd ~/releases/prod/equities/releases/<timestamp>-shared-nodes
uv sync --all-groups --all-extras --frozen
```

Expected: success with a release-local `.venv/bin/python`

**Step 4: Re-render prod systemd envs from the new release**

Run:

```bash
sudo env ROOT_DIR="$PWD" \
  EQUITIES_DEPLOY_ROOT="$PWD" \
  EQUITIES_ENABLE_EXECUTION=1 \
  bash "$PWD/ops/scripts/deploy/install_equities_systemd.sh"
```

Expected:

- `/etc/flux/equities-node-*.env` count drops to `19`
- `flux-equities.target` wants `19` grouped node services
- `/etc/sudoers.d/flux-pulse` references grouped node services only
- no env file points at a mutable checkout or worktree

**Step 5: Verify the rendered operator surface before restart**

Run:

```bash
find /etc/flux -maxdepth 1 -type f -name 'equities-node-*.env' -print | sort
sed -n '1,200p' /etc/systemd/system/flux-equities.target
sed -n '1,200p' /etc/sudoers.d/flux-pulse
```

Expected:

- exactly `19` grouped node envs
- no stale `equities-node-<strategy_id>.env` files
- target membership and sudoers entries match the grouped node ids exactly

**Step 6: Stop legacy per-strategy nodes and clear failed state**

Run:

```bash
sudo systemctl stop 'flux@equities-node-*.service'
sudo systemctl reset-failed 'flux@equities-node-*.service'
```

Expected: no old per-strategy node service remains active before grouped nodes are started.

**Step 7: Restart the equities stack in dependency order**

Run:

```bash
sudo systemctl restart flux@equities-portfolio.service
sudo systemctl restart flux@equities-bridge.service
sudo systemctl start flux-equities.target
sudo systemctl restart flux@equities-api.service
```

Expected: all grouped node services active and running from the new release root.

Do not restart `chainsaw@md-ibkr-publisher.service` as part of the grouped-node cutover unless the release changes its config or the preflight shows publisher drift. This plan is about node topology first, not hidden publisher restarts.

**Step 8: Verify live Pulse state**

Run:

```bash
curl -fsS 'http://127.0.0.1:5024/api/pulse/jobs'
```

Expected:

- the `equities` Pulse group contains the grouped node job ids, not the retired per-strategy job ids
- grouped node jobs report healthy/expected statuses
- operator actions remain available for the grouped node job ids through the Pulse API

**Step 9: Verify production behavior**

Run:

```bash
systemctl list-units 'flux@equities-node-*.service' --no-legend --plain
python - <<'PY'
import requests
base='http://127.0.0.1:5024/api/v1'
for path in ['signals','balances','trades','alerts','params','param-schema']:
    r=requests.get(f'{base}/{path}', params={'profile':'equities'}, timeout=10)
    print(path, r.status_code)
    print(r.text[:1200])
PY
```

Expected:

- `19` grouped node services, not `38`
- `38` strategy rows on `signals` and `params`
- representative per-strategy selectors still work for:
  - `aapl_tradexyz_maker`
  - `aapl_tradexyz_taker`
  - `amzn_binance_perp_maker`
  - `amzn_binance_perp_taker`
- `balances` remains a profile-level payload with non-degraded readiness metadata
- `trades` and `alerts` do not leak grouped node ids when rows are present
- `balances.degraded == false`
- `missing_required == []`
- `stale_required == []`
- no persistent `stale_state` rows caused by IBKR handshake exhaustion
- no grouped node id leaks into external strategy payloads

**Step 10: Verify public `/equities` realtime compatibility**

Run:

```bash
curl -fsS 'http://127.0.0.1:5022/equities' >/dev/null
# then verify a fresh page load performs one initial signal snapshot and continues
# receiving realtime updates over the current Socket.IO transport contract
```

Expected:

- `/equities` serves successfully from the public route
- the page continues updating over the existing realtime transport contract
- grouped node ids do not leak into public Fluxboard payloads or display state

**Step 11: Run the fail-closed readiness gate**

Run:

```bash
bash ops/scripts/deploy/check_equities_live_readiness.sh
curl -fsS 'http://127.0.0.1:5024/api/v1/readiness?profile=equities'
```

Expected:

- the scripted readiness gate exits successfully
- `ok == true`
- `failed_checks == []`
- readiness evidence agrees with the balances payload: no `missing_required`, no `stale_required`, no degraded scope surprises

Do not call the grouped-node cutover healthy until this step passes.

**Step 12: Document the exact rollback procedure**

The runbook must state rollback as an atomic revert:

- repoint to the previous immutable release
- rerender `/etc/flux` and `/etc/sudoers.d/flux-pulse`
- stop grouped node units
- restart the previous per-strategy registry
- verify no grouped node service remains active or restartable

**Step 13: Commit**

```bash
git add deploy/equities/README.md docs/runbooks/equities-shared-node-cutover.md docs/reviews
git commit -m "docs: record grouped equities node cutover"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

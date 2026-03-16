# TokenMM Grafana Exporter Dashboards Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.
> **Progress:** The Progress Tracker in this document is the execution source of truth and must be updated on every state change.

**Goal:** Add a minimal Grafana surface for TokenMM by porting the Chainsaw liquidity dashboard and adding a basic durable markouts dashboard backed by local exporters in this repo.

**Architecture:** Recommended approach: copy only the Grafana provisioning scaffold and the relevant TokenMM dashboard shape from `/home/ubuntu/chainsaw`, then add two small repo-local exporters. Use a Redis-backed exporter for quote uptime and quote depth because those metrics live in live strategy state today, and use a SQLite-backed exporter for markouts because `execution_markout` is already the durable source of truth in this repo. Do not pull in the full Chainsaw monitoring stack, exchange-volume polling, or broader warehouse/reporting work for this PR. Most importantly, all Grafana work must stay off the strategy hotpath: no additional logic in quote-cycle execution, no synchronous Prometheus emission from strategy code, no new persistence in live handlers for dashboard-only needs, and no changes that add per-tick or per-fill overhead to trading execution.

**Tech Stack:** Python 3.13, `prometheus_client`, Redis, SQLite, pandas, Grafana JSON dashboards, pytest.

## Progress Tracker

**Source of truth:** Update this table whenever task state changes. Do not rely on memory, chat history, or TodoWrite alone.

| Task | Status | Owner | Notes / Last Update |
| --- | --- | --- | --- |
| Overall | in_progress | main | 2026-03-16: execution started in same session with subagent-driven development; plan updated with explicit off-hotpath guardrails before implementation. Baseline note: `pytest -q --noconftest tests/unit_tests/ops/test_makerv3_markouts.py tests/unit_tests/research/test_telemetry_helpers.py` on clean `origin/main` currently hits 2 existing `flux.api` import failures in the markouts ops slice. |
| Task 1: Create The Minimal Grafana Scaffold And Asset Contract Tests | in_review_quality | code-quality-reviewer | 2026-03-16: spec review passed. RED confirmed on missing monitoring files; scaffold added in main session fallback after implementer handoff did not surface cleanly. Verification so far: `python3 -m pytest -q --noconftest tests/unit_tests/ops/test_grafana_assets.py` -> `3 passed`; YAML parse check -> `yaml-ok`. |
| Task 2: Port The TokenMM Liquidity Exporter And Dashboard | not_started | unassigned | Plan created |
| Task 3: Add The SQLite-Backed TokenMM Markouts Exporter | not_started | unassigned | Plan created |
| Task 4: Add The Basic Markouts Dashboard And Operator Docs | not_started | unassigned | Plan created |

---

## Hotpath Guardrails

These constraints are mandatory for every task in this plan.

- All Grafana and Prometheus logic must run in separate polling sidecars or static asset files, never inline in MakerV3 strategy execution.
- Do not modify strategy quote loops, execution-engine flow, persistence actors, or node startup paths to support dashboards unless the change is strictly configuration or documentation.
- Exporters may read from Redis and SQLite, but they must do so on bounded polling intervals with bounded scan windows.
- Prefer already-published Redis state and already-persisted SQLite tables. If data is not already available cheaply, leave it out of scope for this PR.
- Reviews must reject any change that adds dashboard-driven work to the trade/quote hotpath.

### Task 1: Create The Minimal Grafana Scaffold And Asset Contract Tests

**Files:**
- Create: `monitoring/grafana/provisioning/dashboards/dashboards.yml`
- Create: `monitoring/grafana/provisioning/datasources/datasources.yml`
- Create: `monitoring/DASHBOARDS.md`
- Create: `tests/unit_tests/ops/test_grafana_assets.py`

**Step 1: Write the failing test**

Create `tests/unit_tests/ops/test_grafana_assets.py` with a small contract for the new monitoring surface:

- `monitoring/grafana/provisioning/dashboards/dashboards.yml` exists and points Grafana at `/var/lib/grafana/dashboards`
- `monitoring/grafana/provisioning/datasources/datasources.yml` exists and declares a Prometheus datasource with uid `prometheus`
- `monitoring/DASHBOARDS.md` lists the two intended dashboard files:
  - `tokenmm_liquidity_v1.json`
  - `tokenmm_markouts_v1.json`

Use simple file reads plus YAML parsing so later tasks can extend the same test instead of inventing new one-off checks.

**Step 2: Run test to verify it fails**

Run: `python3 -m pytest -q --noconftest tests/unit_tests/ops/test_grafana_assets.py`

Expected: FAIL because none of the monitoring files exist in this repo on `origin/main`.

**Step 3: Write minimal implementation**

Create the initial monitoring scaffold:

- Copy the provider shape from `/home/ubuntu/chainsaw/monitoring/grafana/provisioning/dashboards/dashboards.yml`
- Add a minimal Prometheus datasource file under `monitoring/grafana/provisioning/datasources/datasources.yml`
- Add `monitoring/DASHBOARDS.md` documenting:
  - purpose of the repo-local Grafana assets
  - provisioning paths
  - validation commands
  - the two dashboard filenames planned for this PR

Keep this task limited to scaffolding and documentation. Do not create dashboard JSON files yet.

**Step 4: Run tests to verify it passes**

Run:

- `python3 -m pytest -q --noconftest tests/unit_tests/ops/test_grafana_assets.py`
- `python3 - <<'PY'\nimport yaml\nfrom pathlib import Path\nfor path in [\n    Path('monitoring/grafana/provisioning/dashboards/dashboards.yml'),\n    Path('monitoring/grafana/provisioning/datasources/datasources.yml'),\n]:\n    yaml.safe_load(path.read_text(encoding='utf-8'))\nprint('yaml-ok')\nPY`

Expected: PASS.

**Step 5: Commit**

```bash
git add monitoring/grafana/provisioning/dashboards/dashboards.yml \
  monitoring/grafana/provisioning/datasources/datasources.yml \
  monitoring/DASHBOARDS.md \
  tests/unit_tests/ops/test_grafana_assets.py
git commit -m "feat(monitoring): add minimal grafana scaffold"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 2: Port The TokenMM Liquidity Exporter And Dashboard

**Files:**
- Create: `ops/scripts/exporters/tokenmm_metrics_exporter.py`
- Create: `monitoring/grafana/dashboards/tokenmm_liquidity_v1.json`
- Modify: `monitoring/DASHBOARDS.md`
- Modify: `tests/unit_tests/ops/test_grafana_assets.py`
- Create: `tests/unit_tests/ops/test_tokenmm_metrics_exporter.py`

**Step 1: Write the failing test**

Create `tests/unit_tests/ops/test_tokenmm_metrics_exporter.py` by porting the narrowest useful subset of `/home/ubuntu/chainsaw/tests/unit/exporters/test_tokenmm_metrics_exporter.py`.

Cover only the metrics needed for the requested liquidity dashboard:

- `compute_quote_up(...)`
- `compute_depth_usd_within_bps(...)`
- strategy context discovery / normalization
- exporter polling from `maker_arb:{strategy_id}:state`
- exported gauges for:
  - `tokenmm_quote_up`
  - `tokenmm_quote_depth_usd_100bps`
  - `tokenmm_quote_depth_usd_200bps`
- a contract that the exporter reads existing Redis state only and does not require any changes under `flux/strategies/` or `systems/flux/flux/runners/`

Also extend `tests/unit_tests/ops/test_grafana_assets.py` so it expects:

- `monitoring/grafana/dashboards/tokenmm_liquidity_v1.json`
- dashboard uid `tokenmm-liquidity-v1`
- at least one table panel and one time-series panel
- PromQL queries reference the `tokenmm_quote_up` and `tokenmm_quote_depth_usd_*` metrics, not the original `chainsaw_*` names

**Step 2: Run tests to verify it fails**

Run:

- `python3 -m pytest -q --noconftest tests/unit_tests/ops/test_tokenmm_metrics_exporter.py`
- `python3 -m pytest -q --noconftest tests/unit_tests/ops/test_grafana_assets.py`

Expected: FAIL because the exporter and dashboard do not exist yet.

**Step 3: Write minimal implementation**

Create `ops/scripts/exporters/tokenmm_metrics_exporter.py` by adapting `/home/ubuntu/chainsaw/scripts/exporters/tokenmm_metrics_exporter.py` with a hard scope cut:

- keep quote-up and quote-depth logic
- keep strategy context discovery if it helps label stability
- drop exchange-volume fetches, market-share calculations, and other daily report logic
- expose a small HTTP server with `prometheus_client`
- read only existing Redis state such as `maker_arb:{strategy_id}:state`
- do not modify strategy publishers, quote-cycle payloads, or persistence surfaces for dashboard convenience
- default to bounded-cardinality labels:
  - `env`
  - `token`
  - `venue`
  - `symbol`
  - `strategy_family`
- keep polling bounded and explicit, for example with exporter-local intervals and bounded scans only

Create `monitoring/grafana/dashboards/tokenmm_liquidity_v1.json` by copying `/home/ubuntu/chainsaw/monitoring/grafana/dashboards/tokenmm_client_mm_v1.json` and trimming it to the requested minimal surface:

- average uptime table
- average depth table / joined table view
- quote uptime time series
- quote depth time series

Remove market-share and exchange-volume panels from this repo’s version unless they can be satisfied without widening the exporter scope.

**Step 4: Run tests to verify it passes**

Run:

- `python3 -m pytest -q --noconftest tests/unit_tests/ops/test_tokenmm_metrics_exporter.py`
- `python3 -m pytest -q --noconftest tests/unit_tests/ops/test_grafana_assets.py`
- `python3 -m json.tool monitoring/grafana/dashboards/tokenmm_liquidity_v1.json >/dev/null`
- `python3 ops/scripts/exporters/tokenmm_metrics_exporter.py --help`

Expected: PASS.

**Step 5: Commit**

```bash
git add ops/scripts/exporters/tokenmm_metrics_exporter.py \
  monitoring/grafana/dashboards/tokenmm_liquidity_v1.json \
  monitoring/DASHBOARDS.md \
  tests/unit_tests/ops/test_grafana_assets.py \
  tests/unit_tests/ops/test_tokenmm_metrics_exporter.py
git commit -m "feat(tokenmm): add grafana liquidity exporter"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 3: Add The SQLite-Backed TokenMM Markouts Exporter

**Files:**
- Create: `ops/scripts/exporters/tokenmm_markouts_exporter.py`
- Create: `tests/unit_tests/ops/test_tokenmm_markouts_exporter.py`

**Step 1: Write the failing test**

Create `tests/unit_tests/ops/test_tokenmm_markouts_exporter.py` using temporary SQLite fixtures for `execution_fill` and `execution_markout`.

Cover:

- loading the local telemetry DBs from the default TokenMM path or explicit CLI flags
- joining markouts back to fills on the durable fill key (`trader_id + event_id`)
- grouping by `strategy_id`, `venue`, `symbol`, `order_side`, and `horizon_s`
- exporting gauges for:
  - `tokenmm_markout_avg_bps`
  - `tokenmm_markout_nw_bps`
  - `tokenmm_markout_resolved_rows`
  - `tokenmm_markout_fill_count`
  - `tokenmm_markout_resolution_rate`
  - `tokenmm_markout_last_target_ts_seconds`
- a contract that the exporter computes dashboard aggregates from existing SQLite state only and does not require any new markout persistence fields for Grafana

Use a bounded trailing window argument such as `--window-hours 24` so the exporter remains cheap to poll.

**Step 2: Run test to verify it fails**

Run: `python3 -m pytest -q --noconftest tests/unit_tests/ops/test_tokenmm_markouts_exporter.py`

Expected: FAIL because the exporter does not exist yet.

**Step 3: Write minimal implementation**

Create `ops/scripts/exporters/tokenmm_markouts_exporter.py` as a standalone polling exporter.

Implementation constraints:

- read `fills.sqlite` and `markouts.sqlite`
- reuse `research/tokenmm/telemetry_helpers.py` for enrichment / fill-key logic where practical
- keep the exporter read-only
- compute one aggregated row per label tuple rather than one metric per fill
- default the benchmark to `fv_market_mid`
- ignore unresolved rows when computing average / notional-weighted markout gauges
- do not change markout persistence actors, strategy logic, or live publishing paths for dashboard-only aggregation
- bound reads by trailing time window and aggregation cardinality so polling cost is predictable

Suggested label set:

- `env`
- `profile`
- `strategy_id`
- `venue`
- `symbol`
- `order_side`
- `horizon_s`
- `benchmark_name`

Keep the exporter separate from the liquidity exporter so each sidecar has one data source and one failure mode.

**Step 4: Run tests to verify it passes**

Run:

- `python3 -m pytest -q --noconftest tests/unit_tests/ops/test_tokenmm_markouts_exporter.py`
- `python3 ops/scripts/exporters/tokenmm_markouts_exporter.py --help`

Expected: PASS.

**Step 5: Commit**

```bash
git add ops/scripts/exporters/tokenmm_markouts_exporter.py \
  tests/unit_tests/ops/test_tokenmm_markouts_exporter.py
git commit -m "feat(markouts): add grafana sqlite exporter"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 4: Add The Basic Markouts Dashboard And Operator Docs

**Files:**
- Create: `monitoring/grafana/dashboards/tokenmm_markouts_v1.json`
- Modify: `monitoring/DASHBOARDS.md`
- Modify: `tests/unit_tests/ops/test_grafana_assets.py`
- Modify: `docs/runbooks/makerv3-markouts.md`

**Step 1: Write the failing test**

Extend `tests/unit_tests/ops/test_grafana_assets.py` so it also validates the new markouts dashboard:

- file exists at `monitoring/grafana/dashboards/tokenmm_markouts_v1.json`
- dashboard uid is `tokenmm-markouts-v1`
- queries reference the `tokenmm_markout_*` metrics
- panels cover at least:
  - strategy/horizon markout table
  - resolution-rate table or stat
  - recent markout trend or per-side comparison

Also add a small docs assertion that `docs/runbooks/makerv3-markouts.md` mentions the new exporter and dashboard assets.

**Step 2: Run test to verify it fails**

Run: `python3 -m pytest -q --noconftest tests/unit_tests/ops/test_grafana_assets.py`

Expected: FAIL because the markouts dashboard and exporter docs do not exist yet.

**Step 3: Write minimal implementation**

Create `monitoring/grafana/dashboards/tokenmm_markouts_v1.json` with a minimal operator surface:

- table: average resolved markout bps by `strategy_id` and `horizon_s`
- table or stat: resolution rate by `strategy_id` and `horizon_s`
- time series or bar chart: per-side markout or resolved-row trend over the selected window

Keep the dashboard narrow:

- no warehouse dependencies
- no notebook-only fields
- no per-fill visualization
- no long-horizon FV overlays from the demo notebook

Update `monitoring/DASHBOARDS.md` and `docs/runbooks/makerv3-markouts.md` with:

- exporter command / port
- datasource expectations
- dashboard filenames
- validation commands
- an explicit note that dashboards are sidecar-only and intentionally off the trading hotpath

**Step 4: Run tests to verify it passes**

Run:

- `python3 -m pytest -q --noconftest tests/unit_tests/ops/test_grafana_assets.py`
- `python3 -m json.tool monitoring/grafana/dashboards/tokenmm_markouts_v1.json >/dev/null`
- `python3 -m json.tool monitoring/grafana/dashboards/tokenmm_liquidity_v1.json >/dev/null`

Expected: PASS.

**Step 5: Commit**

```bash
git add monitoring/grafana/dashboards/tokenmm_markouts_v1.json \
  monitoring/DASHBOARDS.md \
  tests/unit_tests/ops/test_grafana_assets.py \
  docs/runbooks/makerv3-markouts.md
git commit -m "feat(grafana): add tokenmm markouts dashboard"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

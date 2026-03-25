# TokenMM Markouts Dashboard Hardening Implementation Plan

> **For the execution agent:** REQUIRED SUB-SKILL: Before implementing this plan, choose exactly one execution mode and use the matching skill: `superpowers:subagent-driven-development` for same-session execution or `superpowers:executing-plans` for a separate-session handoff.
> **Progress:** The Progress Tracker in this document is the execution source of truth and must be updated on every state change.

**Goal:** Make the TokenMM Grafana markouts dashboard materially usable in production by fixing broken filters, aligning time-window semantics, surfacing the exporter’s full useful label/metric set, and adding regression coverage for those behaviors.

**Architecture:** Keep the existing off-hotpath model: Grafana reads Prometheus metrics from `tokenmm_markouts_exporter.py`, and the exporter continues reading bounded SQLite snapshots. Do not move any dashboard logic into strategy execution. The main dashboard changes are data-driven templating, query rewrites that respect the chosen filters consistently, and a broader operator surface that matches the exporter’s available dimensions (`venue`, `symbol`, `order_side`, `horizon_s`, `benchmark_name`) and metrics (`avg_bps`, `nw_bps`, `resolved_rows`, `fill_count`, `resolution_rate`, `last_target_ts_seconds`).

**Tech Stack:** Grafana JSON dashboards, Prometheus/PromQL, Python 3.12+, pytest, existing TokenMM markouts exporter.

## Progress Tracker

**Source of truth:** Update this table whenever task state changes. Do not rely on memory, chat history, or TodoWrite alone.

| Task | Status | Owner | Depends On | Write Scope | Lane Branch | Worktree Path | Commit / Diff | Verification | Notes / Last Update |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Task 1: Audit And Redesign Markouts Variable Model | completed | main | none | `monitoring/grafana/dashboards/tokenmm_markouts_v1.json`, `tests/unit_tests/ops/test_grafana_assets.py` | `codex/tokenmm-markouts-dashboard-hardening-20260325` | `/home/ubuntu/nautilus_trader/.worktrees/tokenmm-markouts-dashboard-hardening-20260325` | working_tree | `python3 -m pytest -q --noconftest tests/unit_tests/ops/test_grafana_assets.py -k markouts PASS` | 2026-03-25: real `strategy_id`/`venue`/`symbol`/`order_side`/`horizon_s`/`benchmark_name`/`window` filter contract implemented |
| Task 2: Rewrite Dashboard Queries And Panels Around Working Filters | completed | main | Task 1: Audit And Redesign Markouts Variable Model | `monitoring/grafana/dashboards/tokenmm_markouts_v1.json`, `tests/unit_tests/ops/test_grafana_assets.py` | `codex/tokenmm-markouts-dashboard-hardening-20260325` | `/home/ubuntu/nautilus_trader/.worktrees/tokenmm-markouts-dashboard-hardening-20260325` | working_tree | `python3 -m pytest -q --noconftest tests/unit_tests/ops/test_grafana_assets.py PASS` | 2026-03-25: replaced horizon-pivot snapshot with filter-driven detail table and fully scoped legends/queries |
| Task 3: Add Operator Health Panels Missing From The Current Board | completed | main | Task 2: Rewrite Dashboard Queries And Panels Around Working Filters | `monitoring/grafana/dashboards/tokenmm_markouts_v1.json`, `tests/unit_tests/ops/test_grafana_assets.py`, `monitoring/DASHBOARDS.md`, `docs/runbooks/makerv3-markouts.md` | `codex/tokenmm-markouts-dashboard-hardening-20260325` | `/home/ubuntu/nautilus_trader/.worktrees/tokenmm-markouts-dashboard-hardening-20260325` | working_tree | `python3 -m json.tool monitoring/grafana/dashboards/tokenmm_markouts_v1.json >/dev/null PASS` | 2026-03-25: added weighted markout, fill coverage, and last-target-age operator surfaces plus docs |
| Task 4: Add Regression Coverage For Template Variables And Query Scope | completed | main | Task 1: Audit And Redesign Markouts Variable Model | `tests/unit_tests/ops/test_grafana_assets.py` | `codex/tokenmm-markouts-dashboard-hardening-20260325` | `/home/ubuntu/nautilus_trader/.worktrees/tokenmm-markouts-dashboard-hardening-20260325` | working_tree | `python3 -m pytest -q --noconftest tests/unit_tests/ops/test_grafana_assets.py PASS` | 2026-03-25: asset tests now lock variable presence, query scoping, legends, and missing markout metrics |
| Task 5: Verify Against A Live Grafana/Prometheus Stack | blocked | main | Task 2: Rewrite Dashboard Queries And Panels Around Working Filters, Task 3: Add Operator Health Panels Missing From The Current Board, Task 4: Add Regression Coverage For Template Variables And Query Scope | `monitoring/grafana/dashboards/tokenmm_markouts_v1.json`, `tests/unit_tests/ops/test_grafana_assets.py` | `codex/tokenmm-markouts-dashboard-hardening-20260325` | `/home/ubuntu/nautilus_trader/.worktrees/tokenmm-markouts-dashboard-hardening-20260325` | none | `python3 -m pytest -q --noconftest tests/unit_tests/ops/test_tokenmm_markouts_exporter.py PASS; git diff --check PASS` | 2026-03-25: repo-side verification complete, but live Grafana import/deploy access is not available from this session |

---

### Task 1: Audit And Redesign Markouts Variable Model

**Files:**
- Modify: `monitoring/grafana/dashboards/tokenmm_markouts_v1.json`
- Modify: `tests/unit_tests/ops/test_grafana_assets.py`

**Dependencies:** `none`

**Write Scope:** `monitoring/grafana/dashboards/tokenmm_markouts_v1.json`, `tests/unit_tests/ops/test_grafana_assets.py`

**Verification Commands:**
- `python3 -m json.tool monitoring/grafana/dashboards/tokenmm_markouts_v1.json >/dev/null`
- `python3 -m pytest -q --noconftest tests/unit_tests/ops/test_grafana_assets.py -k markouts`

**Step 1: Write the failing tests for variable coverage**

Extend `tests/unit_tests/ops/test_grafana_assets.py` so the markouts dashboard must contain:

- a variable for `strategy_id`
- a variable for `venue`
- a variable for `symbol`
- a variable for `order_side`
- a variable for `horizon_s`
- a variable for `benchmark_name`
- a variable for `window`

Also assert that `order_side`, `horizon_s`, and `benchmark_name` are not wildcard-only single-option placeholders.

**Step 2: Run tests to verify they fail**

Run: `python3 -m pytest -q --noconftest tests/unit_tests/ops/test_grafana_assets.py -k markouts`

Expected: FAIL because the current dashboard only offers wildcard placeholders for `strategy_id`, `order_side`, and `horizon_s`, and does not expose `venue` or `symbol`.

**Step 3: Replace inert variables with an intentional filter model**

Update `monitoring/grafana/dashboards/tokenmm_markouts_v1.json` to:

- keep `env` and `profile` as controlled custom selectors
- add `venue` and `symbol` variables because the exporter labels already expose them
- replace wildcard-only `order_side` and `horizon_s` with explicit selectable values
- decide whether `benchmark_name` should stay explicit or become multi-select if operators need side-by-side comparison
- make `window` clearly mean one thing:
  - either a snapshot aggregation window only, while dashboard time range stays separate
  - or the primary dashboard time range control, with panel queries rewritten to use it consistently

Document the intended semantics in the variable descriptions or dashboard panel descriptions so operators understand the difference between “dashboard timerange” and “aggregation window” if both remain.

**Step 4: Run tests to verify they pass**

Run:

- `python3 -m json.tool monitoring/grafana/dashboards/tokenmm_markouts_v1.json >/dev/null`
- `python3 -m pytest -q --noconftest tests/unit_tests/ops/test_grafana_assets.py -k markouts`

Expected: PASS.

**Step 5: Commit**

```bash
git add monitoring/grafana/dashboards/tokenmm_markouts_v1.json \
  tests/unit_tests/ops/test_grafana_assets.py
git commit -m "fix(markouts): make dashboard filters real"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 2: Rewrite Dashboard Queries And Panels Around Working Filters

**Files:**
- Modify: `monitoring/grafana/dashboards/tokenmm_markouts_v1.json`
- Modify: `tests/unit_tests/ops/test_grafana_assets.py`

**Dependencies:** `Task 1: Audit And Redesign Markouts Variable Model`

**Write Scope:** `monitoring/grafana/dashboards/tokenmm_markouts_v1.json`, `tests/unit_tests/ops/test_grafana_assets.py`

**Verification Commands:**
- `python3 -m json.tool monitoring/grafana/dashboards/tokenmm_markouts_v1.json >/dev/null`
- `python3 -m pytest -q --noconftest tests/unit_tests/ops/test_grafana_assets.py -k markouts`

**Step 1: Write failing tests for query scope**

Add test assertions that the main markouts panels include the chosen variable set in their PromQL:

- snapshot panel queries must scope by `strategy_id`, `venue`, `symbol`, `order_side`, and `benchmark_name`
- time-series panels must scope by the same labels
- if `horizon_s` remains a variable for time-series panels, its query use must be asserted

Also add a test that the legend format contains enough information to disambiguate lines when multiple venues, symbols, or benchmarks are visible.

**Step 2: Run tests to verify they fail**

Run: `python3 -m pytest -q --noconftest tests/unit_tests/ops/test_grafana_assets.py -k markouts`

Expected: FAIL because the current board ignores `venue` and `symbol`, and its legends collapse distinct series into identical labels.

**Step 3: Rewrite the dashboard queries**

Update the current panels so they actually behave well when filters are used:

- Snapshot panel:
  - include `venue` and `symbol` in grouping and display
  - choose a stable join key that does not collapse distinct filtered series
  - avoid synthetic grouping that becomes ambiguous once more dimensions are added
- Average markout panel:
  - include all active label filters
  - use a legend such as `{{strategy_id}} {{venue}} {{symbol}} {{horizon_s}}s {{order_side}} {{benchmark_name}}`
- Resolution and count panels:
  - use the same label scope and legend policy

If the snapshot becomes too wide once `venue` and `symbol` are added, split it into a summary table and a detail table instead of keeping one overloaded panel.

**Step 4: Run tests to verify they pass**

Run:

- `python3 -m json.tool monitoring/grafana/dashboards/tokenmm_markouts_v1.json >/dev/null`
- `python3 -m pytest -q --noconftest tests/unit_tests/ops/test_grafana_assets.py -k markouts`

Expected: PASS.

**Step 5: Commit**

```bash
git add monitoring/grafana/dashboards/tokenmm_markouts_v1.json \
  tests/unit_tests/ops/test_grafana_assets.py
git commit -m "fix(markouts): align panel queries with dashboard filters"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 3: Add Operator Health Panels Missing From The Current Board

**Files:**
- Modify: `monitoring/grafana/dashboards/tokenmm_markouts_v1.json`
- Modify: `tests/unit_tests/ops/test_grafana_assets.py`
- Modify: `monitoring/DASHBOARDS.md`
- Modify: `docs/runbooks/makerv3-markouts.md`

**Dependencies:** `Task 2: Rewrite Dashboard Queries And Panels Around Working Filters`

**Write Scope:** `monitoring/grafana/dashboards/tokenmm_markouts_v1.json`, `tests/unit_tests/ops/test_grafana_assets.py`, `monitoring/DASHBOARDS.md`, `docs/runbooks/makerv3-markouts.md`

**Verification Commands:**
- `python3 -m json.tool monitoring/grafana/dashboards/tokenmm_markouts_v1.json >/dev/null`
- `python3 -m pytest -q --noconftest tests/unit_tests/ops/test_grafana_assets.py -k markouts`

**Step 1: Write failing tests for missing metrics**

Extend asset tests so the dashboard must use:

- `tokenmm_markout_nw_bps`
- `tokenmm_markout_last_target_ts_seconds`

and must document the board’s purpose beyond simple averages.

**Step 2: Run tests to verify they fail**

Run: `python3 -m pytest -q --noconftest tests/unit_tests/ops/test_grafana_assets.py -k markouts`

Expected: FAIL because the current dashboard never surfaces notional-weighted markouts or freshness.

**Step 3: Add the missing operator panels**

Add panels that close the biggest operator blind spots:

- notional-weighted markout panel, because raw average bps overweights tiny fills
- freshness/staleness panel using `tokenmm_markout_last_target_ts_seconds`
- an at-a-glance fill or resolution health panel that helps distinguish “bad performance” from “no resolved data”

Keep the board compact, but make it answer:

- how markouts are performing
- whether enough data exists to trust the numbers
- whether the exporter is fresh

Update `monitoring/DASHBOARDS.md` and `docs/runbooks/makerv3-markouts.md` with the new panel surface and the intended operator interpretation.

**Step 4: Run tests to verify they pass**

Run:

- `python3 -m json.tool monitoring/grafana/dashboards/tokenmm_markouts_v1.json >/dev/null`
- `python3 -m pytest -q --noconftest tests/unit_tests/ops/test_grafana_assets.py -k markouts`

Expected: PASS.

**Step 5: Commit**

```bash
git add monitoring/grafana/dashboards/tokenmm_markouts_v1.json \
  tests/unit_tests/ops/test_grafana_assets.py \
  monitoring/DASHBOARDS.md \
  docs/runbooks/makerv3-markouts.md
git commit -m "feat(markouts): add weighted and freshness panels"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 4: Add Regression Coverage For Template Variables And Query Scope

**Files:**
- Modify: `tests/unit_tests/ops/test_grafana_assets.py`

**Dependencies:** `Task 1: Audit And Redesign Markouts Variable Model`

**Write Scope:** `tests/unit_tests/ops/test_grafana_assets.py`

**Verification Commands:**
- `python3 -m pytest -q --noconftest tests/unit_tests/ops/test_grafana_assets.py`

**Step 1: Write the missing contract cases**

Add targeted assertions for:

- markouts variable names and option/query shapes
- panel legends containing disambiguating labels
- panel expressions referencing every supported filter dimension
- the presence of `tokenmm_markout_nw_bps` and `tokenmm_markout_last_target_ts_seconds`
- a dashboard default time setting that matches the selected window semantics

**Step 2: Run tests to verify they fail on the old dashboard**

Run: `python3 -m pytest -q --noconftest tests/unit_tests/ops/test_grafana_assets.py`

Expected: FAIL against the old JSON.

**Step 3: Keep only durable contract assertions**

Avoid brittle tests for full raw JSON blobs. Assert operator-facing contracts instead:

- filters exist
- filters are wired into queries
- critical metrics are surfaced
- legends remain unambiguous

**Step 4: Run tests to verify they pass**

Run: `python3 -m pytest -q --noconftest tests/unit_tests/ops/test_grafana_assets.py`

Expected: PASS.

**Step 5: Commit**

```bash
git add tests/unit_tests/ops/test_grafana_assets.py
git commit -m "test(markouts): cover dashboard filter and metric contracts"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 5: Verify Against A Live Grafana/Prometheus Stack

**Files:**
- Modify: `monitoring/grafana/dashboards/tokenmm_markouts_v1.json`
- Modify: `tests/unit_tests/ops/test_grafana_assets.py`

**Dependencies:** `Task 2: Rewrite Dashboard Queries And Panels Around Working Filters`, `Task 3: Add Operator Health Panels Missing From The Current Board`, `Task 4: Add Regression Coverage For Template Variables And Query Scope`

**Write Scope:** `monitoring/grafana/dashboards/tokenmm_markouts_v1.json`, `tests/unit_tests/ops/test_grafana_assets.py`

**Verification Commands:**
- `python3 -m pytest -q --noconftest tests/unit_tests/ops/test_grafana_assets.py`
- `python3 -m json.tool monitoring/grafana/dashboards/tokenmm_markouts_v1.json >/dev/null`

**Step 1: Verify the dashboard JSON locally**

Run:

- `python3 -m json.tool monitoring/grafana/dashboards/tokenmm_markouts_v1.json >/dev/null`
- `python3 -m pytest -q --noconftest tests/unit_tests/ops/test_grafana_assets.py`

Expected: PASS.

**Step 2: Load the dashboard into a real Grafana instance**

Use the normal repo deployment path or Grafana import flow to load the updated JSON. Then manually verify:

- `window` changes affect every panel that claims to use it
- `order_side` can switch between `BUY` and `SELL`
- `horizon_s` can isolate `0/30/60/120`
- `venue` and `symbol` filters behave as expected
- legends stay readable when multiple series are selected

**Step 3: Compare against exporter output**

Cross-check at least one filtered view against raw Prometheus series or the exporter `/metrics` surface so the dashboard matches the label tuples actually emitted.

**Step 4: Fix any live-only issues**

If Grafana-specific quirks appear after import, make the minimal JSON adjustment and rerun the local validation commands.

**Step 5: Commit**

```bash
git add monitoring/grafana/dashboards/tokenmm_markouts_v1.json \
  tests/unit_tests/ops/test_grafana_assets.py
git commit -m "chore(markouts): verify dashboard in live grafana"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

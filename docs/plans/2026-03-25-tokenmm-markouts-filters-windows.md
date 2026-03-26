# TokenMM Markouts Filters And Windows Implementation Plan

> **For the execution agent:** REQUIRED SUB-SKILL: Before implementing this plan, choose exactly one execution mode and use the matching skill: `superpowers:subagent-driven-development` for same-session execution or `superpowers:executing-plans` for a separate-session handoff.
> **Progress:** The Progress Tracker in this document is the execution source of truth and must be updated on every state change.

**Goal:** Make all live markouts dashboard filters truthful, convert benchmark selection to single-select, and expand true exported analysis windows through `1w`.

**Architecture:** Keep the current sidecar-exporter architecture and extend the exporter’s `analysis_window` contract. Grafana remains a filtered view over exported gauges, while the exporter becomes the single source of truth for supported windows and benchmark semantics.

**Tech Stack:** Python exporter, Prometheus gauges, Grafana dashboard JSON, pytest asset tests, systemd-managed live sidecars.

## Progress Tracker

**Source of truth:** Update this table whenever task state changes. Do not rely on memory, chat history, or TodoWrite alone.

| Task | Status | Owner | Depends On | Write Scope | Lane Branch | Worktree Path | Commit / Diff | Verification | Notes / Last Update |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Task 1: Extend exported analysis windows | completed | main | none | `ops/scripts/exporters/tokenmm_markouts_exporter.py`, `tests/unit_tests/ops/test_tokenmm_markouts_exporter.py` | `shared` | `shared` | none | `python3 -m pytest -q --noconftest tests/unit_tests/ops/test_tokenmm_markouts_exporter.py PASS` | Exporter now emits `15m,1h,2h,4h,1d,2d,3d,1w` and enforces `168h` minimum |
| Task 2: Fix dashboard variable semantics | completed | main | Task 1: Extend exported analysis windows | `monitoring/grafana/dashboards/tokenmm_markouts_v1.json`, `tests/unit_tests/ops/test_grafana_assets.py` | `shared` | `shared` | none | `python3 -m pytest -q --noconftest tests/unit_tests/ops/test_grafana_assets.py PASS` | Benchmark is single-select, window options expanded, default moved to `2h` |
| Task 3: Update docs and runtime contract | completed | main | Task 1: Extend exported analysis windows | `monitoring/DASHBOARDS.md`, `docs/runbooks/makerv3-markouts.md` | `shared` | `shared` | none | `rg -n '1w|2h|single-select|analysis_window' monitoring/DASHBOARDS.md docs/runbooks/makerv3-markouts.md PASS` | Docs updated for `1w` bounded read and truthful filter semantics |
| Task 4: Verify branch state | completed | main | Task 1: Extend exported analysis windows, Task 2: Fix dashboard variable semantics, Task 3: Update docs and runtime contract | `tests/unit_tests/ops/test_tokenmm_markouts_exporter.py`, `tests/unit_tests/ops/test_grafana_assets.py`, `monitoring/grafana/dashboards/tokenmm_markouts_v1.json` | `shared` | `shared` | none | `pytest ops suites PASS; json.tool PASS; git diff --check PASS` | Branch verification complete; ready for PR and live rollout |
| Task 5: Merge and deploy live | not_started | main | Task 4: Verify branch state | `/home/ubuntu/nautilus_trader`, `/etc/tokenmm-monitoring/grafana/dashboards/tokenmm_markouts_v1.json`, live services | `shared` | `shared` | none | not_run | Plan created |

---

### Task 1: Extend Exported Analysis Windows

**Files:**
- Modify: `ops/scripts/exporters/tokenmm_markouts_exporter.py`
- Test: `tests/unit_tests/ops/test_tokenmm_markouts_exporter.py`

**Dependencies:** `none`

**Write Scope:** `ops/scripts/exporters/tokenmm_markouts_exporter.py`, `tests/unit_tests/ops/test_tokenmm_markouts_exporter.py`

**Verification Commands:**
- `python3 -m pytest -q --noconftest tests/unit_tests/ops/test_tokenmm_markouts_exporter.py`

**Step 1: Write failing exporter tests**

Add assertions for:

- supported `analysis_window` values being exactly `15m`, `1h`, `2h`, `4h`, `1d`, `2d`, `3d`, `1w`
- parser/runtime validation requiring at least `168h`
- emitted metrics including longer windows when data exists

**Step 2: Run test to verify it fails**

Run: `python3 -m pytest -q --noconftest tests/unit_tests/ops/test_tokenmm_markouts_exporter.py`
Expected: FAIL on old window set and old max-window validation.

**Step 3: Write minimal exporter implementation**

Update:

- `ANALYSIS_WINDOWS`
- `MAX_ANALYSIS_WINDOW_HOURS`
- help text and validation messaging
- any tests/helpers assuming `24h` is the max window label

**Step 4: Run test to verify it passes**

Run: `python3 -m pytest -q --noconftest tests/unit_tests/ops/test_tokenmm_markouts_exporter.py`
Expected: PASS

**Step 5: Commit**

```bash
git add ops/scripts/exporters/tokenmm_markouts_exporter.py tests/unit_tests/ops/test_tokenmm_markouts_exporter.py
git commit -m "feat(markouts): expand exported analysis windows"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 2: Fix Dashboard Variable Semantics

**Files:**
- Modify: `monitoring/grafana/dashboards/tokenmm_markouts_v1.json`
- Test: `tests/unit_tests/ops/test_grafana_assets.py`

**Dependencies:** `Task 1: Extend exported analysis windows`

**Write Scope:** `monitoring/grafana/dashboards/tokenmm_markouts_v1.json`, `tests/unit_tests/ops/test_grafana_assets.py`

**Verification Commands:**
- `python3 -m pytest -q --noconftest tests/unit_tests/ops/test_grafana_assets.py`
- `python3 -m json.tool monitoring/grafana/dashboards/tokenmm_markouts_v1.json >/dev/null`

**Step 1: Write failing dashboard asset tests**

Add assertions for:

- `benchmark_name` being single-select with no `All`
- default benchmark being `fv_market_mid`
- `window` options being exactly `15m,1h,2h,4h,1d,2d,3d,1w`
- default window being `2h`
- all panel queries retaining live filter scope

**Step 2: Run test to verify it fails**

Run: `python3 -m pytest -q --noconftest tests/unit_tests/ops/test_grafana_assets.py`
Expected: FAIL on current benchmark/window variable configuration.

**Step 3: Write minimal dashboard implementation**

Update dashboard variables and any query/legend logic required so:

- benchmark selection is single-select only
- snapshot table remains `Strategy | Side`
- all filter semantics remain truthful

**Step 4: Run tests to verify they pass**

Run:

- `python3 -m pytest -q --noconftest tests/unit_tests/ops/test_grafana_assets.py`
- `python3 -m json.tool monitoring/grafana/dashboards/tokenmm_markouts_v1.json >/dev/null`

Expected: PASS

**Step 5: Commit**

```bash
git add monitoring/grafana/dashboards/tokenmm_markouts_v1.json tests/unit_tests/ops/test_grafana_assets.py
git commit -m "fix(markouts): make dashboard filters truthful"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 3: Update Docs And Runtime Contract

**Files:**
- Modify: `monitoring/DASHBOARDS.md`
- Modify: `docs/runbooks/makerv3-markouts.md`

**Dependencies:** `Task 1: Extend exported analysis windows`

**Write Scope:** `monitoring/DASHBOARDS.md`, `docs/runbooks/makerv3-markouts.md`

**Verification Commands:**
- `rg -n '1w|2h|benchmark|single-select|analysis_window' monitoring/DASHBOARDS.md docs/runbooks/makerv3-markouts.md`

**Step 1: Update operator docs**

Document:

- benchmark single-select semantics
- the new supported analysis windows
- runtime requirement that `--window-hours` cover `1w`

**Step 2: Verify docs mention the new contract**

Run: `rg -n '1w|2h|benchmark|single-select|analysis_window' monitoring/DASHBOARDS.md docs/runbooks/makerv3-markouts.md`
Expected: matches in both files.

**Step 3: Commit**

```bash
git add monitoring/DASHBOARDS.md docs/runbooks/makerv3-markouts.md
git commit -m "docs(markouts): document filter and window contract"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 4: Verify Branch State

**Files:**
- Test: `tests/unit_tests/ops/test_tokenmm_markouts_exporter.py`
- Test: `tests/unit_tests/ops/test_grafana_assets.py`
- Verify: `monitoring/grafana/dashboards/tokenmm_markouts_v1.json`

**Dependencies:** `Task 1: Extend exported analysis windows`, `Task 2: Fix Dashboard Variable Semantics`, `Task 3: Update Docs And Runtime Contract`

**Write Scope:** none

**Verification Commands:**
- `python3 -m pytest -q --noconftest tests/unit_tests/ops/test_tokenmm_markouts_exporter.py tests/unit_tests/ops/test_grafana_assets.py`
- `python3 -m json.tool monitoring/grafana/dashboards/tokenmm_markouts_v1.json >/dev/null`
- `git diff --check`

**Step 1: Run full relevant verification**

Run all commands above and confirm green state.

**Step 2: Commit any tracker/doc updates if needed**

```bash
git add docs/plans/2026-03-25-tokenmm-markouts-filters-windows.md
git commit -m "docs(plans): update markouts filters and windows tracker"
```

Only commit if the plan tracker changed materially.

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 5: Merge And Deploy Live

**Files:**
- Modify via deploy: `/etc/tokenmm-monitoring/grafana/dashboards/tokenmm_markouts_v1.json`
- Service runtime: `flux@tokenmm-markouts-exporter.service`, `flux@tokenmm-grafana.service`

**Dependencies:** `Task 4: Verify Branch State`

**Write Scope:** live branch, PR state, deployed dashboard JSON, live services

**Verification Commands:**
- `gh pr create ...` / `gh pr merge ...`
- `systemctl status flux@tokenmm-markouts-exporter.service --no-pager --lines=2`
- `systemctl status flux@tokenmm-grafana.service --no-pager --lines=2`
- `curl -sf http://127.0.0.1:9109/metrics | grep 'analysis_window=' | head`
- Grafana dashboard API verification

**Step 1: Open and merge the PR**

Use the feature branch from this worktree and merge after verification.

**Step 2: Deploy the updated dashboard JSON and restart services**

Copy the dashboard JSON to `/etc/tokenmm-monitoring/grafana/dashboards/tokenmm_markouts_v1.json`, restart the exporter and Grafana services, and re-import the dashboard with overwrite if provisioning does not immediately reflect the new version.

**Step 3: Verify live behavior**

Confirm:

- live metrics emit the expanded `analysis_window` values when data exists
- dashboard variable config reflects single-select benchmark and `2h` default
- services are healthy

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

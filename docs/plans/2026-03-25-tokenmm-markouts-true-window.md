# TokenMM Markouts True Window Implementation Plan

> **For the execution agent:** REQUIRED SUB-SKILL: Before implementing this plan, choose exactly one execution mode and use the matching skill: `superpowers:subagent-driven-development` for same-session execution or `superpowers:executing-plans` for a separate-session handoff.
> **Progress:** The Progress Tracker in this document is the execution source of truth and must be updated on every state change.

**Goal:** Make the TokenMM markouts dashboard `window` selector choose a true exported analysis window (`15m`, `1h`, `4h`, `24h`) instead of applying a range function to already-aggregated gauges.

**Architecture:** Keep the current SQLite -> exporter -> Prometheus -> Grafana flow. Change the exporter to publish one aggregate snapshot per configured analysis window using a new `analysis_window` label, then update the Grafana dashboard to filter on that label directly and let Grafana's time picker control only chart history.

**Tech Stack:** Python, Prometheus client gauges, Grafana dashboard JSON, pytest

## Progress Tracker

**Source of truth:** Update this table whenever task state changes. Do not rely on memory, chat history, or TodoWrite alone.

| Task | Status | Owner | Depends On | Write Scope | Lane Branch | Worktree Path | Commit / Diff | Verification | Notes / Last Update |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Overall | completed | main | none | `ops/scripts/exporters/tokenmm_markouts_exporter.py`, `monitoring/grafana/dashboards/tokenmm_markouts_v1.json`, `tests/unit_tests/ops/test_tokenmm_markouts_exporter.py`, `tests/unit_tests/ops/test_grafana_assets.py`, `monitoring/DASHBOARDS.md`, `docs/runbooks/makerv3-markouts.md` | `shared` | `shared` | `363dce5092..working_tree` | `python3 -m pytest -q --noconftest tests/unit_tests/ops/test_tokenmm_markouts_exporter.py tests/unit_tests/ops/test_grafana_assets.py` PASS | Spec review passed and final quality review returned no findings |
| Task 1: Add failing exporter tests for true analysis windows | completed | main | none | `tests/unit_tests/ops/test_tokenmm_markouts_exporter.py` | `shared` | `shared` | none | `python3 -m pytest -q --noconftest tests/unit_tests/ops/test_tokenmm_markouts_exporter.py -k analysis_windows` FAIL | Failing contract test added; exporter missing `ANALYSIS_WINDOWS` as expected |
| Task 2: Export per-window markout gauges with an `analysis_window` label | completed | main | Task 1: Add failing exporter tests for true analysis windows | `ops/scripts/exporters/tokenmm_markouts_exporter.py`, `tests/unit_tests/ops/test_tokenmm_markouts_exporter.py` | `shared` | `shared` | none | `python3 -m pytest -q --noconftest tests/unit_tests/ops/test_tokenmm_markouts_exporter.py` PASS | Exporter now emits configured fixed windows with `analysis_window` labels |
| Task 3: Add failing Grafana asset tests for the new window semantics | completed | main | Task 2: Export per-window markout gauges with an `analysis_window` label | `tests/unit_tests/ops/test_grafana_assets.py` | `shared` | `shared` | none | `python3 -m pytest -q --noconftest tests/unit_tests/ops/test_grafana_assets.py -k analysis_window_selector` FAIL | Failing dashboard contract test added for direct `analysis_window` selection |
| Task 4: Rewire the dashboard to select `analysis_window` directly | completed | main | Task 3: Add failing Grafana asset tests for the new window semantics | `monitoring/grafana/dashboards/tokenmm_markouts_v1.json`, `tests/unit_tests/ops/test_grafana_assets.py` | `shared` | `shared` | none | `python3 -m pytest -q --noconftest tests/unit_tests/ops/test_grafana_assets.py -k 'markouts_dashboard_uses_window_as_analysis_window_selector or markouts_snapshot_table_pivots_horizons_into_ordered_columns or markouts_dashboard_benchmark_selector_offers_fv_and_local_mid'` PASS | Dashboard now filters on `analysis_window=~\"$window\"` and no longer uses range functions for window semantics |
| Task 5: Update operator docs for true window semantics | completed | main | Task 4: Rewire the dashboard to select `analysis_window` directly | `monitoring/DASHBOARDS.md`, `docs/runbooks/makerv3-markouts.md` | `shared` | `shared` | none | `rg -n "analysis_window|15m|1h|4h|24h|window" monitoring/DASHBOARDS.md docs/runbooks/makerv3-markouts.md` PASS | Docs now describe exported fixed windows and time-picker semantics |
| Task 6: Run full verification and prepare live rollout | completed | main | Task 5: Update operator docs for true window semantics | `ops/scripts/exporters/tokenmm_markouts_exporter.py`, `monitoring/grafana/dashboards/tokenmm_markouts_v1.json`, `tests/unit_tests/ops/test_tokenmm_markouts_exporter.py`, `tests/unit_tests/ops/test_grafana_assets.py`, `monitoring/DASHBOARDS.md`, `docs/runbooks/makerv3-markouts.md` | `shared` | `shared` | `363dce5092..working_tree` | `python3 -m pytest -q --noconftest tests/unit_tests/ops/test_tokenmm_markouts_exporter.py tests/unit_tests/ops/test_grafana_assets.py` PASS; `python3 -m json.tool monitoring/grafana/dashboards/tokenmm_markouts_v1.json >/dev/null` PASS; `git diff --check` PASS; `python3 ops/scripts/exporters/tokenmm_markouts_exporter.py --help | rg -n 'window-hours|24h|analysis window'` PASS; `python3 ops/scripts/exporters/tokenmm_markouts_exporter.py --window-hours 1 --poll-interval-s 1` exits 2 with argparse error; `ps -ef | rg 'tokenmm_markouts_exporter.py'` shows `--window-hours 24` | Spec review passed and final quality review returned no findings |

---

### Task 1: Add failing exporter tests for true analysis windows

**Files:**
- Modify: `tests/unit_tests/ops/test_tokenmm_markouts_exporter.py`

**Dependencies:** `none`

**Write Scope:** `tests/unit_tests/ops/test_tokenmm_markouts_exporter.py`

**Verification Commands:**
- `python3 -m pytest -q --noconftest tests/unit_tests/ops/test_tokenmm_markouts_exporter.py -k analysis_window`
- `python3 -m pytest -q --noconftest tests/unit_tests/ops/test_tokenmm_markouts_exporter.py`

**Steps:**
1. Add failing tests for the `analysis_window` label and the fixed window set `15m`, `1h`, `4h`, `24h`.
2. Use fixture data that makes at least one tuple differ between a short window and `24h`.
3. Run the focused exporter test and verify it fails for the missing contract.
4. Commit the failing-test state.

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 2: Export per-window markout gauges with an `analysis_window` label

**Files:**
- Modify: `ops/scripts/exporters/tokenmm_markouts_exporter.py`
- Modify: `tests/unit_tests/ops/test_tokenmm_markouts_exporter.py`

**Dependencies:** `Task 1: Add failing exporter tests for true analysis windows`

**Write Scope:** `ops/scripts/exporters/tokenmm_markouts_exporter.py`, `tests/unit_tests/ops/test_tokenmm_markouts_exporter.py`

**Verification Commands:**
- `python3 -m pytest -q --noconftest tests/unit_tests/ops/test_tokenmm_markouts_exporter.py -k analysis_window`
- `python3 -m pytest -q --noconftest tests/unit_tests/ops/test_tokenmm_markouts_exporter.py`

**Steps:**
1. Define the configurable analysis windows in one place, for example a tuple of `(label, hours)` pairs.
2. Add `analysis_window` to the exported label contract.
3. In `poll_once(...)`, compute one snapshot per configured window.
4. Publish values under the existing metric families with the new label attached.
5. Keep stale-series cleanup correct across all windows.
6. Run the focused and full exporter tests until they pass.
7. Commit the exporter implementation.

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 3: Add failing Grafana asset tests for the new window semantics

**Files:**
- Modify: `tests/unit_tests/ops/test_grafana_assets.py`

**Dependencies:** `Task 2: Export per-window markout gauges with an `analysis_window` label`

**Write Scope:** `tests/unit_tests/ops/test_grafana_assets.py`

**Verification Commands:**
- `python3 -m pytest -q --noconftest tests/unit_tests/ops/test_grafana_assets.py -k markouts`

**Steps:**
1. Add failing assertions that the `window` variable still offers `15m`, `1h`, `4h`, `24h`.
2. Assert the panel queries filter on `analysis_window=~"$window"`.
3. Assert the snapshot and chart queries stop using range-vector functions for analysis window semantics.
4. Run the focused dashboard test and verify it fails.
5. Commit the failing-test state.

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 4: Rewire the dashboard to select `analysis_window` directly

**Files:**
- Modify: `monitoring/grafana/dashboards/tokenmm_markouts_v1.json`
- Modify: `tests/unit_tests/ops/test_grafana_assets.py`

**Dependencies:** `Task 3: Add failing Grafana asset tests for the new window semantics`

**Write Scope:** `monitoring/grafana/dashboards/tokenmm_markouts_v1.json`, `tests/unit_tests/ops/test_grafana_assets.py`

**Verification Commands:**
- `python3 -m pytest -q --noconftest tests/unit_tests/ops/test_grafana_assets.py -k markouts`
- `python3 -m json.tool monitoring/grafana/dashboards/tokenmm_markouts_v1.json >/dev/null`

**Steps:**
1. Keep the operator-facing variable name `window`.
2. Rework the snapshot table to select the chosen `analysis_window` directly while preserving `Strategy | Side` and the fixed `0s / 30s / 60s / 120s` columns.
3. Rework the charts to plot gauge history directly over the dashboard time range and filter on `analysis_window=~"$window"`.
4. Keep the current filter scope and clear legends.
5. Run focused dashboard tests and JSON validation until they pass.
6. Commit the dashboard change.

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 5: Update operator docs for true window semantics

**Files:**
- Modify: `monitoring/DASHBOARDS.md`
- Modify: `docs/runbooks/makerv3-markouts.md`

**Dependencies:** `Task 4: Rewire the dashboard to select `analysis_window` directly`

**Write Scope:** `monitoring/DASHBOARDS.md`, `docs/runbooks/makerv3-markouts.md`

**Verification Commands:**
- `rg -n "analysis_window|15m|1h|4h|24h|window" monitoring/DASHBOARDS.md docs/runbooks/makerv3-markouts.md`

**Steps:**
1. Document the exporter’s new `analysis_window` label contract.
2. Document that `window` now selects the analysis window while Grafana’s time picker controls chart history.
3. Document that changing the supported windows is a code/config change.
4. Run the grep check and commit the doc update.

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 6: Run full verification and prepare live rollout

**Files:**
- Modify: `ops/scripts/exporters/tokenmm_markouts_exporter.py`
- Modify: `monitoring/grafana/dashboards/tokenmm_markouts_v1.json`
- Modify: `tests/unit_tests/ops/test_tokenmm_markouts_exporter.py`
- Modify: `tests/unit_tests/ops/test_grafana_assets.py`
- Modify: `monitoring/DASHBOARDS.md`
- Modify: `docs/runbooks/makerv3-markouts.md`

**Dependencies:** `Task 5: Update operator docs for true window semantics`

**Write Scope:** `ops/scripts/exporters/tokenmm_markouts_exporter.py`, `monitoring/grafana/dashboards/tokenmm_markouts_v1.json`, `tests/unit_tests/ops/test_tokenmm_markouts_exporter.py`, `tests/unit_tests/ops/test_grafana_assets.py`, `monitoring/DASHBOARDS.md`, `docs/runbooks/makerv3-markouts.md`

**Verification Commands:**
- `python3 -m pytest -q --noconftest tests/unit_tests/ops/test_tokenmm_markouts_exporter.py tests/unit_tests/ops/test_grafana_assets.py`
- `python3 -m json.tool monitoring/grafana/dashboards/tokenmm_markouts_v1.json >/dev/null`
- `git diff --check`

**Steps:**
1. Run the full targeted test suite.
2. Re-run dashboard JSON validation.
3. Re-run diff hygiene.
4. Record the rollout sequence: restart the markouts exporter, redeploy/re-import the dashboard, verify live `analysis_window` series, and verify the live dashboard changes when switching `window`.
5. Commit the verified state.

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

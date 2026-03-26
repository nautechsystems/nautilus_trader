# TokenMM Liquidity Per-Strategy Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.
> **Progress:** The Progress Tracker in this document is the execution source of truth and must be updated on every state change.

**Goal:** Convert the TokenMM liquidity exporter and Grafana dashboard from market-level aggregation to true per-strategy observability.

**Architecture:** Add `strategy_id` to the existing liquidity metric label schema, then update the dashboard queries and tests to join and render rows by strategy rather than `symbol@venue`. Keep the existing dashboard UID and the current non-hotpath exporter polling model.

**Tech Stack:** Python exporter, Prometheus metrics, Grafana JSON dashboards, pytest asset/unit tests

## Progress Tracker

**Source of truth:** Update this table whenever task state changes. Do not rely on memory, chat history, or TodoWrite alone.

| Task | Status | Owner | Notes / Last Update |
| --- | --- | --- | --- |
| Overall | in_progress | main | Implementation and targeted verification complete; preparing commit and PR |
| Task 1: Add failing per-strategy liquidity tests | completed | main | Exporter and dashboard tests updated for `strategy_id` labels and strategy-scoped joins |
| Task 2: Implement strategy-scoped liquidity metrics | completed | main | Added `strategy_id` label to liquidity metrics and lightweight FluxRedisKeys fallback |
| Task 3: Convert liquidity dashboard to strategy-first layout | completed | main | Dashboard queries, joins, legends, and filters now pivot on `strategy_id` |
| Task 4: Update docs and verify targeted suite | completed | main | `23 passed`; dashboard JSON parses; exporter `--help` and `git diff --check` clean |
| Task 5: Commit and open PR | in_progress | main | Commit and PR creation pending |

---

### Task 1: Add failing per-strategy liquidity tests

**Files:**
- Modify: `tests/unit_tests/ops/test_tokenmm_metrics_exporter.py`
- Modify: `tests/unit_tests/ops/test_grafana_assets.py`

Add failing tests that lock the new metric/dashboard contract:
- exporter samples must include `strategy_id`
- two strategies on the same venue/symbol produce distinct series
- dashboard snapshot table/time series join by `strategy_id`

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 2: Implement strategy-scoped liquidity metrics

**Files:**
- Modify: `ops/scripts/exporters/tokenmm_metrics_exporter.py`

Extend the liquidity metric label schema with `strategy_id` and update label construction, preservation, and metric sync logic without changing polling cadence or hotpath behavior.

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 3: Convert liquidity dashboard to strategy-first layout

**Files:**
- Modify: `monitoring/grafana/dashboards/tokenmm_liquidity_v1.json`

Replace market-level joins and legends with strategy-level joins, rename the primary table column to `Strategy`, and preserve the existing dashboard UID.

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 4: Update docs and verify targeted suite

**Files:**
- Modify: `monitoring/DASHBOARDS.md`
- Modify: any liquidity-facing runbook or dashboard asset tests as needed

Document the strategy-scoped liquidity semantics and note that live deploys must not hard-pin a three-strategy allowlist if full TokenMM coverage is expected. Run targeted exporter/dashboard verification.

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 5: Commit and open PR

**Files:**
- No new product files

Create a focused commit for the per-strategy liquidity change and open a PR against `main` with the deploy note about updating the live Pulse unit after merge.

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

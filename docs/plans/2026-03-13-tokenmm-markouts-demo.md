# TokenMM Markouts Demo Notebook Implementation Plan

> **For the execution agent:** REQUIRED SUB-SKILL: Before implementing this plan, choose exactly one execution mode and use the matching skill: `superpowers:subagent-driven-development` for same-session execution or `superpowers:executing-plans` for a separate-session handoff.
> **Progress:** The Progress Tracker in this document is the execution source of truth and must be updated on every state change.

**Goal:** Build a SQLite-first demo notebook for Jeff that shows live FV markouts, side splits, notional weighting, compact pivots, optional frozen-FV edge analysis, and a caveated current-mark PnL context.

**Architecture:** Keep the implementation notebook-friendly. A small helper module will centralize SQLite loading, parsing, and aggregation logic; a small extraction script will freeze optional FV stream history; the notebook will orchestrate both and degrade cleanly when no extract is present.

**Tech Stack:** Python 3, pandas, sqlite3, redis-py, JSON notebooks

## Progress Tracker

**Source of truth:** Update this table whenever task state changes. Do not rely on memory, chat history, or TodoWrite alone.

| Task | Status | Owner | Depends On | Write Scope | Lane Branch | Worktree Path | Commit / Diff | Verification | Notes / Last Update |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Task 1: Add helper tests and module | completed | main | none | `tests/unit_tests/research/test_telemetry_helpers.py`, `research/tokenmm/telemetry_helpers.py` | shared | shared | none | `python3 -m pytest tests/unit_tests/research/test_telemetry_helpers.py -q PASS` | Helper module implemented and green on targeted unit tests |
| Task 2: Add Redis FV freeze script | completed | main | Task 1: Add helper tests and module | `ops/scripts/export_tokenmm_markout_inputs.py` | shared | shared | none | `python3 ops/scripts/export_tokenmm_markout_inputs.py --help PASS` | Script implemented with config-aware Redis URL defaults and password overrides |
| Task 3: Build demo notebook | completed | main | Task 1: Add helper tests and module | `research/tokenmm/notebooks/tokenmm_markouts_edge_pnl_demo.ipynb` | shared | shared | none | `python3 notebook cell executor PASS` | Notebook built with SQLite-first core and optional frozen-FV sections |
| Task 4: Validate notebook and spot-check outputs | completed | main | Task 2: Add Redis FV freeze script, Task 3: Build demo notebook | repo-local validation only | shared | shared | none | `json parse PASS; notebook exec PASS; raw SQLite spot-check PASS` | Notebook JSON validated, code cells executed end-to-end on local snapshot, sample fill/markout joins spot-checked against raw SQLite rows |

---

### Task 1: Add helper tests and module

**Files:**
- Create: `tests/unit_tests/research/test_telemetry_helpers.py`
- Create: `research/tokenmm/telemetry_helpers.py`

**Dependencies:** `none`

**Write Scope:** `tests/unit_tests/research/test_telemetry_helpers.py`, `research/tokenmm/telemetry_helpers.py`

**Verification Commands:**
- `python3 -m pytest tests/unit_tests/research/test_telemetry_helpers.py -q`

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 2: Add Redis FV freeze script

**Files:**
- Create: `ops/scripts/export_tokenmm_markout_inputs.py`

**Dependencies:** `Task 1: Add helper tests and module`

**Write Scope:** `ops/scripts/export_tokenmm_markout_inputs.py`

**Verification Commands:**
- `python3 ops/scripts/export_tokenmm_markout_inputs.py --help`

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 3: Build demo notebook

**Files:**
- Create: `research/tokenmm/notebooks/tokenmm_markouts_edge_pnl_demo.ipynb`

**Dependencies:** `Task 1: Add helper tests and module`

**Write Scope:** `research/tokenmm/notebooks/tokenmm_markouts_edge_pnl_demo.ipynb`

**Verification Commands:**
- `python3 - <<'PY' ... parse notebook json ... PY`
- `python3 - <<'PY' ... execute notebook source cells against local SQLite snapshot ... PY`

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 4: Validate notebook and spot-check outputs

**Files:**
- Modify: `docs/plans/2026-03-13-tokenmm-markouts-demo.md`

**Dependencies:** `Task 2: Add Redis FV freeze script`, `Task 3: Build demo notebook`

**Write Scope:** `docs/plans/2026-03-13-tokenmm-markouts-demo.md`

**Verification Commands:**
- `python3 -m pytest tests/unit_tests/research/test_telemetry_helpers.py -q`
- `python3 ops/scripts/export_tokenmm_markout_inputs.py --help`
- `python3 - <<'PY' ... validate + execute notebook ... PY`

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

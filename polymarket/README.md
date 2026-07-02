# Polymarket research/backtest v1

This directory is a repo-local Polymarket research framework.  It is not a
production NautilusTrader adapter package.

## Boundaries

- `adapters/` own source compatibility for PMXT parquet, PMXT curated event
  folders, local live raw WebSocket captures, and future event bundles.
- `models.py` defines the canonical L2 replay data consumed by the backtest.
- `backtest_v1.py` is the single v1 run/backtest entry point.
- `research/` contains isolated strategy, data-validation, and smoke-test
  experiments.
- `ideas/` contains long-horizon brainstorms.
- `boss_reports/` contains polished decision reports.

Documented CLI form:

```powershell
python -m polymarket.backtest_v1 --config polymarket/research/<topic>/experiment.yml
```

Run commands from the repository root so `polymarket.*` absolute imports are
stable.

## Source status

- `pmxt_parquet_v1`: legacy/questionable.  PMXT parquet lacks raw WebSocket
  message boundaries and source timestamps may invert.
- `pmxt_event_v1`: legacy/questionable because it is derived from PMXT data.
- `live_ws_v1`: preferred current path for local raw WebSocket captures.
- `live_event_bundle_v1`: future data-team event bundle boundary.

## Experiment convention

Each `research/<date-topic>/` directory should own its `experiment.yml`,
`strategy.py`, `report.md`, and representative `runs/<run_id>/` outputs.  Runs
must write both `original_config.yml` and `resolved_config.json`.

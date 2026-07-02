# T02 PMXT smoke (v1 framework)

This directory contains the migrated T02 PMXT received-time smoke sample.

## v1 entry

```powershell
python -m polymarket.backtest_v1 --config polymarket/research/2026-07-01-t02-smoke-pmxt/experiment.yml
```

The v1 smoke uses the migrated repo-local curated event folder:

```text
polymarket/research/2026-07-01-t02-smoke-pmxt/curated/t02-pmxt-live-until-1100-no-receive-inversion/
```

The PMXT source remains legacy/questionable. The purpose of this smoke is to verify that the new adapter/backtest/config framework can replay the curated PMXT sample without depending on external absolute paths.

## Baseline

The pre-reorg baseline summary is preserved at:

```text
polymarket/research/2026-07-01-t02-smoke-pmxt/data/t02_strategy_suite_summary.csv
```

Use the T02 baseline comparison helper under `polymarket/tests/` to compare new v1 results against the pre-reorg smoke where applicable.

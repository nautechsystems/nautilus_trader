# Shanghai PMXT event research

Migrated from the original root-level `research/2026-06-24-polymarket-shanghai-event-backtest/`
workspace.

This folder preserves the original PMXT-derived Shanghai event research reports, compact
summaries, and charts for historical comparison. The old pre-v1 runner scripts/tests were
not kept as active entry points; new experiments should use:

```powershell
python -m polymarket.backtest_v1 --config polymarket/research/<topic>/experiment.yml
```

PMXT-derived data should be treated as legacy/questionable while the v1 framework moves
toward adapter-isolated replay and local live raw WS verification.

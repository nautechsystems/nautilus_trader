# TokenMM Markouts Filters And Windows Design

**Date:** 2026-03-25

## Goal

Make every markouts dashboard filter truthful and operator-safe, with special focus on:

- `benchmark_name` behaving as a real selector instead of silently averaging benchmarks together
- `window` selecting real exported analysis windows rather than ad hoc Grafana range functions
- expanding historical analysis windows to `15m`, `1h`, `2h`, `4h`, `1d`, `2d`, `3d`, and `1w`

## Current Problems

1. The exporter emits distinct `benchmark_name` series, but the snapshot table groups results back down to `strategy_id, order_side`. When Grafana allows `benchmark_name=All` or multi-select, the panel averages benchmarks together and hides the distinction.
2. The dashboard now uses true `analysis_window` labels, but the supported fixed set is still too narrow for operator workflows.
3. The exporter bounded read only guarantees coverage up to `24h`, so it cannot support longer windows honestly.

## Chosen Design

### 1. Benchmark Filter Becomes Single-Select

The `benchmark_name` dashboard variable will become a single-select custom variable with values:

- `fv_market_mid`
- `local_mkt_mid`

Default will be `fv_market_mid`.

This keeps the existing `Strategy | Side` snapshot layout while ensuring benchmark changes actually change the backing series instead of getting averaged back together.

### 2. Analysis Windows Become Exporter Contract

The exporter will publish exactly these `analysis_window` label values:

- `15m`
- `1h`
- `2h`
- `4h`
- `1d`
- `2d`
- `3d`
- `1w`

The dashboard `window` selector will bind directly to these labels and default to `2h`.

### 3. Bounded Read Must Cover The Largest Window

The exporter currently enforces that `--window-hours` must be at least the maximum supported analysis window. That contract will remain, but the maximum will move from `24h` to `1w` (`168h`).

This keeps the sidecar honest:

- Grafana never recomputes raw windows
- the exporter explicitly controls which analysis windows are available
- runtime configuration must cover the largest exported window

### 4. All Filters Must Remain Live Across All Panels

Every markouts query will continue to honor:

- `strategy_id`
- `venue`
- `symbol`
- `order_side`
- `benchmark_name`
- `window`

The snapshot panel will intentionally not use `horizon_s` because its purpose is to compare fixed `0s / 30s / 60s / 120s` columns side by side.

The time-series and health panels will continue to honor `horizon_s`.

## Tradeoffs

### Recommended Approach: extend the existing labeled-window exporter

Pros:

- smallest operational change
- preserves the current sidecar + Prometheus + Grafana architecture
- makes filters truthful without redesigning storage/query infrastructure

Cons:

- increases series count and exporter compute
- requires widening the bounded SQLite read to one week

### Rejected Alternatives

Separate metric names per window:

- too much metric sprawl
- worse dashboard maintenance

Raw Grafana queries over markout storage:

- larger architectural redesign
- unnecessary for the current operator need

## Success Criteria

- selecting `fv_market_mid` versus `local_mkt_mid` changes the dashboard data rather than averaging both
- the `window` selector offers exactly `15m`, `1h`, `2h`, `4h`, `1d`, `2d`, `3d`, `1w`
- the live default window is `2h`
- exporter/runtime validation requires a bounded read large enough for `1w`
- tests catch benchmark-variable shape, supported windows/default, and query filter coverage

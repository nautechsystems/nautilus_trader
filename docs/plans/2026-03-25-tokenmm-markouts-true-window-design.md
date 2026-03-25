# TokenMM Markouts True Window Design

**Goal:** Make the Grafana `window` control select a true markout analysis window instead of smoothing a pre-aggregated exporter gauge.

## Context

The current path is SQLite -> `tokenmm_markouts_exporter.py` -> Prometheus gauges -> Grafana. That architecture should stay in place because it keeps the dashboard off the trading hot path and off direct SQLite reads.

## Problem

Production currently runs the exporter with one trailing poll window, `--window-hours 24`. The exporter computes one aggregate gauge per label tuple, then Grafana applies `avg_over_time(...[$window])` or `max_over_time(...[$window])` on top of the history of that already-aggregated gauge.

So the dashboard `window` control does not mean “recompute markouts over the selected trailing interval from raw data.” It only smooths the history of a pre-windowed gauge.

## Decision

Keep the sidecar exporter model, but publish true fixed-window aggregates for a configurable set of analysis windows.

Initial window set:
- `15m`
- `1h`
- `4h`
- `24h`

The set must stay configurable in code so a later window change does not require another metric-schema rewrite.

## Metric Contract

Add an `analysis_window` label to every TokenMM markouts gauge. Example:

```text
tokenmm_markout_avg_bps{..., benchmark_name="fv_market_mid", analysis_window="1h"}
```

On each poll, the exporter computes one aggregate snapshot per configured analysis window and publishes all of them under the existing metric families.

This is preferred over metric-name suffixes because it keeps the schema compact, keeps Grafana variable wiring simple, and makes future window changes a label-value change rather than a metric-family explosion.

## Dashboard Semantics

The operator-facing variable can stay named `window`, but it changes meaning:
- old: range-function width on a pre-aggregated gauge
- new: selected `analysis_window` label value

Grafana's time picker remains chart history.

### Snapshot Table

The top table should keep `Strategy | Side` on the left and fixed `0s / 30s / 60s / 120s` horizon columns, but it should query the selected `analysis_window` directly with no extra `avg_over_time(...[$window])`.

### Time-Series Panels

The charts should filter on `analysis_window=~"$window"` and plot the history of the exporter gauges directly over the dashboard time range. That cleanly separates analysis window from chart history.

## Testing

Required coverage:
1. Exporter tests prove all configured windows are emitted and can differ when fixture data differs by time span.
2. Grafana asset tests prove panel queries select `analysis_window` directly and stop using range functions to fake window semantics.
3. Dashboard JSON still parses.
4. Live verification proves changing `window` changes the selected exported aggregate series.

## Rollout

1. Merge exporter and dashboard changes together.
2. Restart the markouts exporter so Prometheus starts scraping `analysis_window`-labeled series.
3. Reload or re-import the Grafana dashboard.
4. Verify live Prometheus series expose all configured windows and the Grafana table changes when switching `window`.

## Non-Goals

- arbitrary free-form analysis windows in Grafana
- raw SQLite queries from Grafana
- hot-path telemetry redesign
- warehouse / Parquet / Postgres work

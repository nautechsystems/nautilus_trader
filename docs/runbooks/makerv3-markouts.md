# MakerV3 Markouts Runbook

This runbook defines the v0 operator contract for MakerV3 markouts.

Use it together with `systems/flux/docs/makerv3.md` and
`deploy/tokenmm/tokenmm.live.toml`.

## What we can compute today

There are two supported paths:

- same-day preliminary numbers from existing Redis streams
- live-forward persistence into SQLite on TokenMM nodes

The same-day report is best-effort and retention-bound. It reads:

- `flux:v1:trades:stream:{strategy_id}`
- `flux:v1:fv:stream:{strategy_id}`

Use it when you need preliminary 30s, 60s, and 120s markouts today and Redis
still retains the needed window.

Example:

```bash
python ops/scripts/makerv3_markouts.py \
  --strategy plumeusdt_bybit_perp_makerv3 \
  --horizons 30,60,120 \
  --json
```

## Live-forward persistence flow

The durable v0 path is live-forward only:

- `events.fills.*` provides the fill anchor
- `flux.makerv3.fv` provides the `fv_market_mid` benchmark for v0
- the local TokenMM node resolves the first FV at or after each target horizon
- the node persists only final rows into `execution_markout`

The actor does not persist raw FV snapshots or any raw live market-data history.

## Join keys and downstream analysis

Use `execution_markout` as the derived markout table and join it back to the
existing persistence surfaces:

- `execution_markout.event_id = execution_fill.event_id`
- `execution_markout.trade_id = execution_fill.trade_id`
- `execution_markout.client_order_id = execution_fill.client_order_id`
- `execution_markout.quote_cycle_id = execution_fill.quote_cycle_id`
- `execution_markout.quote_cycle_id = order_action.quote_cycle_id`
- `execution_markout.run_id` and `quote_cycle_id` let you line the markout back
  up with `quote_cycle`

Prefer `event_id` plus `trader_id` as the durable fill join. `trade_id` is not a
stable unique key across all fills by itself.

## Production and deployment notes

The v0 deployment change is narrow:

- add `markouts_db_path` under `[telemetry_shipper]` in
  `deploy/tokenmm/tokenmm.live.toml`
- set `markout_horizons_s = [0, 30, 60, 120]`
- configure both persisted markout benchmarks:
  `fv_market_mid` from `fv` and `local_mkt_mid` from `maker_mid`
- keep `enable_local_persistence = true`
- restart the existing TokenMM node services so the new actor is enrolled

The new SQLite file path is:

- `markouts_db_path = "/var/lib/nautilus/telemetry/tokenmm/markouts.sqlite"`

No new service, systemd unit, or separate deployment stack is required for v0.
This is an existing TokenMM node runner change, not a new daemon.

Operationally:

- restart TokenMM node jobs after deploying the code and shared config change
- the API service does not need a dedicated markouts process
- `_prepare_telemetry_paths(...)` creates the parent directory for
  `markouts_db_path` automatically on node startup

## Grafana sidecars

The Grafana path for TokenMM markouts is intentionally off the trading hotpath.

- `ops/scripts/exporters/tokenmm_markouts_exporter.py` polls the existing
  `fills.sqlite` and `markouts.sqlite` files and exposes markout performance metrics
  as aggregate Prometheus gauges, including fixed `analysis_window` variants
  for `15m`, `1h`, `2h`, `4h`, `1d`, `2d`, `3d`, and `1w`
- `monitoring/grafana/dashboards/tokenmm_markouts_v1.json` reads those gauges
  for the operator dashboard focused on markout performance by strategy,
  venue, symbol, side, benchmark, and horizon progression
- `ops/scripts/exporters/tokenmm_metrics_exporter.py` and
  `monitoring/grafana/dashboards/tokenmm_liquidity_v1.json` handle the separate
  liquidity and uptime metrics surface

These sidecars do not add work to MakerV3 quote loops or fill handling. They
read existing Redis state and existing SQLite telemetry out of band, off the
trading hotpath.

Recommended minimal sidecar invocation:

```bash
python3 ops/scripts/exporters/tokenmm_markouts_exporter.py \
  --env prod \
  --profile tokenmm \
  --port 9109 \
  --benchmark-name fv_market_mid,local_mkt_mid \
  --poll-interval-s 30 \
  --window-hours 168
```

Keep the polling window bounded. The exporter now rejects non-positive
`--window-hours` values so a bad override cannot silently widen polling into a
full-table scan.
The configured `--window-hours` value must still cover the largest supported
analysis window, currently `1w` (`168h`), so the exported window set stays
fixed.

Dashboard semantics:

- the dashboard `window` selector chooses the exported `analysis_window`
- Grafana's time picker controls how much gauge history the charts show
- changing the supported analysis windows requires an exporter/dashboard code change

Dashboard usage notes:

- the markouts dashboard exposes filterable selectors for `strategy_id`,
  `venue`, `symbol`, `order_side`, `horizon_s`, and `benchmark_name`
- `benchmark_name` is intentionally single-select so the snapshot table stays
  keyed by `Strategy | Side` without averaging benchmarks together
- the `window` selector chooses the exported fixed analysis window, while the
  Grafana time range controls the x-axis span
- use the weighted markout panel when simple average bps is skewed by small fills
- use the last-target-age panel to distinguish stale exporter output from
  genuinely quiet trading periods

## Known limitations and scope

Explicit v0 scope:

- live-forward only
- `fv_market_mid` only
- TokenMM runner only
- raw live market-data history is out of scope
- core Nautilus `streaming` / Parquet catalog capture is a future option, not
  part of this first PR
- Postgres shipper and warehouse integration are out of scope for the first PR

Important limitations:

- the read-only Redis report is retention-bound and should be treated as
  preliminary numbers only
- unresolved live-forward rows remain pending until they resolve, expire, or
  the actor stops
- a clean actor stop persists still-pending horizons with
  `resolution_status="stopped"` so operators can distinguish shutdown from
  true expiry

If a restart happens and Redis retention still covers the window, rerun the
read-only report to recover preliminary numbers for the missed interval.

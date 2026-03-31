# TokenMM Grafana Dashboards

This directory is the repo-local home for the minimal Grafana assets used to
observe TokenMM sidecar exporters.

## Scope

- Grafana provisioning YAML under `monitoring/grafana/provisioning/`
- Checked-in dashboard JSON under `monitoring/grafana/dashboards/`
- Dashboard-only observability assets that remain off the strategy hotpath

The trading system must not emit Prometheus metrics inline from MakerV3 strategy
execution for this surface. Dashboards are expected to read from sidecar
exporters that poll existing Redis state and local SQLite telemetry out of band.

## Dashboard files

- `tokenmm_liquidity_v1.json`
- `tokenmm_markouts_v1.json`

## Current files

- `monitoring/grafana/dashboards/tokenmm_liquidity_v1.json`
- `monitoring/grafana/dashboards/tokenmm_markouts_v1.json`
- `monitoring/grafana/provisioning/dashboards/dashboards.yml`
- `monitoring/grafana/provisioning/datasources/datasources.yml`
- `deploy/tokenmm/systemd/prometheus.yml`
- `deploy/tokenmm/systemd/tokenmm-grafana-run.sh`
- `deploy/tokenmm/systemd/tokenmm-prometheus-run.sh`

## Exporters

- Liquidity sidecar: `python3 ops/scripts/exporters/tokenmm_metrics_exporter.py --help`
  Provides per-strategy liquidity and uptime metrics from existing Redis strategy state.
  Discovers `tokenmm` strategies from `configs/strategies.ini` or falls back to
  the current `plumeusdt_<venue>_<product>_makerv3` allowlist.
- Markouts sidecar: `python3 ops/scripts/exporters/tokenmm_markouts_exporter.py --help`
  Provides markout performance metrics from existing `fills.sqlite` and `markouts.sqlite`.
  Supports multiple benchmarks in one sidecar process, including
  `fv_market_mid` and `local_mkt_mid`. Publishes one aggregate series per
  configured `analysis_window`, currently `15m`, `1h`, `2h`, `4h`, `1d`,
  `2d`, `3d`, and `1w`.

Both sidecars stay off the trading hotpath. They poll existing Redis state and
durable SQLite telemetry out of band instead of emitting metrics inline from
MakerV3 strategy execution.

## Example sidecar commands

```bash
python3 ops/scripts/exporters/tokenmm_metrics_exporter.py \
  --env prod \
  --port 9108 \
  --poll-interval-s 5

python3 ops/scripts/exporters/tokenmm_markouts_exporter.py \
  --env prod \
  --profile tokenmm \
  --port 9109 \
  --benchmark-name fv_market_mid,local_mkt_mid \
  --poll-interval-s 30 \
  --window-hours 24
```

Operational notes:

- both exporters reject `--poll-interval-s` values below `0.5`
- the liquidity dashboard is strategy-scoped, not market-scoped, so future
  TokenMM strategies on the same venue/symbol appear as separate rows and series
- live deploys should avoid hard-pinning `--strategy-id` in the liquidity
  sidecar unit unless intentionally narrowing coverage
- the markouts sidecar can expose `0s`, `30s`, `60s`, and `120s` horizons from
  the same persisted `execution_markout` surface
- the markouts dashboard is filter-driven across `strategy_id`, `venue`,
  `symbol`, `order_side`, `horizon_s`, and `benchmark_name`
- the markouts dashboard now includes notional-weighted markout and freshness
  panels in addition to raw average bps, counts, and resolution rate
- the markouts dashboard keeps `benchmark_name` single-select so the snapshot
  table cannot silently average `fv_market_mid` and `local_mkt_mid` together
- the markouts dashboard `window` selector now chooses the exported
  `analysis_window` label, while Grafana's time picker controls chart history
- changing the supported markouts analysis windows is a code/config change in
  the exporter contract, not an ad hoc Grafana input
- the markouts exporter rejects non-positive `--window-hours` values so the
  bounded trailing-window contract cannot silently turn into a full-table scan
- the markouts exporter must be configured with a `--window-hours` value that
  covers the largest supported analysis window, currently `1w` (`168h`)
- the liquidity exporter keeps polling healthy strategies even if one Redis key
  read fails for a cycle
- the markouts exporter logs and keeps serving if a poll fails because a local
  SQLite file is temporarily missing or locked

## Provisioning paths

- Dashboard provider config:
  `monitoring/grafana/provisioning/dashboards/dashboards.yml`
- Datasource config:
  `monitoring/grafana/provisioning/datasources/datasources.yml`
- Installed host Grafana config root: `/etc/tokenmm-monitoring/grafana`
- Installed host dashboard directory: `/etc/tokenmm-monitoring/grafana/dashboards`
- Installed host Prometheus config: `/etc/tokenmm-monitoring/prometheus/prometheus.yml`
- Installed native Grafana runtime root: `/opt/tokenmm-monitoring/grafana/current`
- Installed native Prometheus runtime root: `/opt/tokenmm-monitoring/prometheus/current`
- Installed wrapper scripts:
  `/usr/local/bin/tokenmm-grafana-run.sh`,
  `/usr/local/bin/tokenmm-prometheus-run.sh`
- Dashboard JSON directory inside Grafana: `/etc/tokenmm-monitoring/grafana/dashboards`

## Validation

```bash
python3 -m pytest -q --noconftest tests/unit_tests/ops/test_grafana_assets.py
python3 -m json.tool monitoring/grafana/dashboards/tokenmm_liquidity_v1.json >/dev/null
python3 -m json.tool monitoring/grafana/dashboards/tokenmm_markouts_v1.json >/dev/null
python3 - <<'PY'
import yaml
from pathlib import Path
for path in [
    Path("monitoring/grafana/provisioning/dashboards/dashboards.yml"),
    Path("monitoring/grafana/provisioning/datasources/datasources.yml"),
]:
    yaml.safe_load(path.read_text(encoding="utf-8"))
print("yaml-ok")
PY
```

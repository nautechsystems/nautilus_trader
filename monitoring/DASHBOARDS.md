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

## Planned dashboards

- `tokenmm_liquidity_v1.json`
- `tokenmm_markouts_v1.json`

## Provisioning paths

- Dashboard provider config:
  `monitoring/grafana/provisioning/dashboards/dashboards.yml`
- Datasource config:
  `monitoring/grafana/provisioning/datasources/datasources.yml`
- Dashboard JSON directory inside Grafana: `/var/lib/grafana/dashboards`

## Validation

```bash
python3 -m pytest -q --noconftest tests/unit_tests/ops/test_grafana_assets.py
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

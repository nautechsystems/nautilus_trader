from __future__ import annotations

from pathlib import Path

import yaml


def _repo_root() -> Path:
    return Path(__file__).resolve().parents[3]


def test_dashboard_provider_points_grafana_at_repo_dashboards_path() -> None:
    path = _repo_root() / "monitoring/grafana/provisioning/dashboards/dashboards.yml"

    payload = yaml.safe_load(path.read_text(encoding="utf-8"))

    providers = payload["providers"]
    assert len(providers) == 1
    provider = providers[0]
    assert provider["type"] == "file"
    assert provider["options"]["path"] == "/var/lib/grafana/dashboards"


def test_prometheus_datasource_uses_stable_uid() -> None:
    path = _repo_root() / "monitoring/grafana/provisioning/datasources/datasources.yml"

    payload = yaml.safe_load(path.read_text(encoding="utf-8"))

    datasources = payload["datasources"]
    assert len(datasources) == 1
    datasource = datasources[0]
    assert datasource["type"] == "prometheus"
    assert datasource["uid"] == "prometheus"


def test_dashboard_catalog_mentions_planned_dashboard_files() -> None:
    path = _repo_root() / "monitoring/DASHBOARDS.md"

    text = path.read_text(encoding="utf-8")

    assert "tokenmm_liquidity_v1.json" in text
    assert "tokenmm_markouts_v1.json" in text

from __future__ import annotations

import json
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


def test_liquidity_dashboard_uses_tokenmm_metric_names() -> None:
    path = _repo_root() / "monitoring/grafana/dashboards/tokenmm_liquidity_v1.json"
    assert path.exists(), "liquidity dashboard JSON should exist"

    payload = json.loads(path.read_text(encoding="utf-8"))

    assert payload["uid"] == "tokenmm-liquidity-v1"
    panel_types = {panel["type"] for panel in payload["panels"]}
    assert "table" in panel_types
    assert "timeseries" in panel_types

    expressions = [
        target["expr"]
        for panel in payload["panels"]
        for target in panel.get("targets", [])
        if "expr" in target
    ]
    assert any("tokenmm_quote_up" in expr for expr in expressions)
    assert any("tokenmm_quote_depth_usd_100bps" in expr for expr in expressions)
    assert any("tokenmm_quote_depth_usd_200bps" in expr for expr in expressions)
    assert all("chainsaw_mm_" not in expr for expr in expressions)


def test_markouts_dashboard_uses_tokenmm_markout_metric_names() -> None:
    path = _repo_root() / "monitoring/grafana/dashboards/tokenmm_markouts_v1.json"
    assert path.exists(), "markouts dashboard JSON should exist"

    payload = json.loads(path.read_text(encoding="utf-8"))

    assert payload["uid"] == "tokenmm-markouts-v1"
    panel_types = {panel["type"] for panel in payload["panels"]}
    assert "table" in panel_types
    assert "timeseries" in panel_types or "stat" in panel_types or "barchart" in panel_types

    expressions = [
        target["expr"]
        for panel in payload["panels"]
        for target in panel.get("targets", [])
        if "expr" in target
    ]
    assert any("tokenmm_markout_avg_bps" in expr for expr in expressions)
    assert any("tokenmm_markout_resolution_rate" in expr for expr in expressions)
    assert any("tokenmm_markout_resolved_rows" in expr for expr in expressions)


def test_markouts_runbook_mentions_grafana_sidecars() -> None:
    runbook = (_repo_root() / "docs/runbooks/makerv3-markouts.md").read_text(encoding="utf-8")
    catalog = (_repo_root() / "monitoring/DASHBOARDS.md").read_text(encoding="utf-8")

    assert "tokenmm_markouts_exporter.py" in runbook
    assert "tokenmm_markouts_v1.json" in runbook
    assert "markout performance metrics" in runbook
    assert "off the trading hotpath" in runbook
    assert "tokenmm_markouts_v1.json" in catalog
    assert "tokenmm_markouts_exporter.py" in catalog
    assert "liquidity and uptime metrics" in catalog
    assert "markout performance metrics" in catalog

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


def test_liquidity_dashboard_is_strategy_scoped() -> None:
    path = _repo_root() / "monitoring/grafana/dashboards/tokenmm_liquidity_v1.json"
    payload = json.loads(path.read_text(encoding="utf-8"))

    snapshot_panel = next(panel for panel in payload["panels"] if panel["id"] == 1)
    assert snapshot_panel["title"] == "Quoting Uptime + Our Quote Depth Quality"

    expressions = [target["expr"] for target in snapshot_panel["targets"]]
    assert all("strategy_id" in expr for expr in expressions)
    assert all("symbol_venue" not in expr for expr in expressions)

    organize = next(
        transformation
        for transformation in snapshot_panel["transformations"]
        if transformation["id"] == "organize"
    )
    rename_by_name = organize["options"]["renameByName"]
    assert rename_by_name["strategy_id"] == "Strategy"

    strategy_override = next(
        override
        for override in snapshot_panel["fieldConfig"]["overrides"]
        if override["matcher"] == {"id": "byName", "options": "Strategy"}
    )
    strategy_width = next(
        prop["value"] for prop in strategy_override["properties"] if prop["id"] == "custom.width"
    )
    assert strategy_width >= 320

    strategy_var = next(
        variable
        for variable in payload["templating"]["list"]
        if variable["name"] == "strategy_id"
    )
    assert strategy_var["query"] == ".*"


def test_markouts_dashboard_exposes_real_filters_and_metric_panels() -> None:
    path = _repo_root() / "monitoring/grafana/dashboards/tokenmm_markouts_v1.json"
    assert path.exists(), "markouts dashboard JSON should exist"

    payload = json.loads(path.read_text(encoding="utf-8"))

    assert payload["uid"] == "tokenmm-markouts-v1"
    variables = {variable["name"]: variable for variable in payload["templating"]["list"]}
    assert {"env", "profile", "strategy_id", "venue", "symbol", "order_side", "horizon_s", "benchmark_name", "window"} <= set(variables)

    strategy_var = variables["strategy_id"]
    venue_var = variables["venue"]
    symbol_var = variables["symbol"]
    order_side_var = variables["order_side"]
    horizon_var = variables["horizon_s"]
    benchmark_var = variables["benchmark_name"]
    window_var = variables["window"]

    for variable in (strategy_var, venue_var, symbol_var):
        assert variable["type"] == "query"
        assert variable["includeAll"] is True
        assert variable["query"].startswith("label_values(")

    assert order_side_var["type"] == "custom"
    assert order_side_var["query"] == "BUY,SELL"
    assert order_side_var["includeAll"] is True

    assert horizon_var["type"] == "custom"
    assert horizon_var["query"] == "0,30,60,120"
    assert horizon_var["includeAll"] is True

    assert benchmark_var["type"] == "custom"
    assert benchmark_var["query"] == "fv_market_mid,local_mkt_mid"
    assert benchmark_var["includeAll"] is True

    assert window_var["type"] == "custom"
    assert window_var["current"]["value"] == "1h"
    assert payload["time"]["from"] == "now-1h"

    panel_types = {panel["type"] for panel in payload["panels"]}
    assert "table" in panel_types
    assert "timeseries" in panel_types

    expressions = [
        target["expr"]
        for panel in payload["panels"]
        for target in panel.get("targets", [])
        if "expr" in target
    ]
    assert any("tokenmm_markout_avg_bps" in expr for expr in expressions)
    assert any("tokenmm_markout_nw_bps" in expr for expr in expressions)
    assert any("tokenmm_markout_resolution_rate" in expr for expr in expressions)
    assert any("tokenmm_markout_resolved_rows" in expr for expr in expressions)
    assert any("tokenmm_markout_fill_count" in expr for expr in expressions)
    assert any("tokenmm_markout_last_target_ts_seconds" in expr for expr in expressions)


def test_markouts_snapshot_table_pivots_horizons_for_strategy_side_rows() -> None:
    path = _repo_root() / "monitoring/grafana/dashboards/tokenmm_markouts_v1.json"
    payload = json.loads(path.read_text(encoding="utf-8"))

    snapshot_panel = next(panel for panel in payload["panels"] if panel["id"] == 1)
    assert snapshot_panel["title"] == "Strategy + Side Snapshot"
    assert len(snapshot_panel["targets"]) == 12

    expressions = [target["expr"] for target in snapshot_panel["targets"]]
    assert any('horizon_s="0"' in expr and "tokenmm_markout_avg_bps" in expr for expr in expressions)
    assert any('horizon_s="30"' in expr and "tokenmm_markout_avg_bps" in expr for expr in expressions)
    assert any('horizon_s="60"' in expr and "tokenmm_markout_avg_bps" in expr for expr in expressions)
    assert any('horizon_s="120"' in expr and "tokenmm_markout_avg_bps" in expr for expr in expressions)
    assert any('horizon_s="0"' in expr and "tokenmm_markout_fill_count" in expr for expr in expressions)
    assert any('horizon_s="30"' in expr and "tokenmm_markout_fill_count" in expr for expr in expressions)
    assert any('horizon_s="60"' in expr and "tokenmm_markout_fill_count" in expr for expr in expressions)
    assert any('horizon_s="120"' in expr and "tokenmm_markout_fill_count" in expr for expr in expressions)
    assert any('horizon_s="0"' in expr and "tokenmm_markout_resolution_rate" in expr for expr in expressions)
    assert any('horizon_s="30"' in expr and "tokenmm_markout_resolution_rate" in expr for expr in expressions)
    assert any('horizon_s="60"' in expr and "tokenmm_markout_resolution_rate" in expr for expr in expressions)
    assert any('horizon_s="120"' in expr and "tokenmm_markout_resolution_rate" in expr for expr in expressions)
    assert all("strategy_id=~\"$strategy_id\"" in expr for expr in expressions)
    assert all("venue=~\"$venue\"" in expr for expr in expressions)
    assert all("symbol=~\"$symbol\"" in expr for expr in expressions)
    assert all("order_side=~\"$order_side\"" in expr for expr in expressions)
    assert all("benchmark_name=~\"$benchmark_name\"" in expr for expr in expressions)
    assert all("horizon_s=~\"$horizon_s\"" not in expr for expr in expressions)
    assert all('label_join(' in expr for expr in expressions)
    assert all('avg by (strategy_id,order_side)' in expr or 'max by (strategy_id,order_side)' in expr for expr in expressions)

    avg_override = next(
        override
        for override in snapshot_panel["fieldConfig"]["overrides"]
        if override["matcher"]["options"] == ".* Avg$"
    )
    avg_unit = next(
        prop["value"] for prop in avg_override["properties"] if prop["id"] == "unit"
    )
    assert avg_unit == "none"

    organize = next(
        transformation
        for transformation in snapshot_panel["transformations"]
        if transformation["id"] == "organize"
    )
    rename_by_name = organize["options"]["renameByName"]
    index_by_name = organize["options"]["indexByName"]

    assert rename_by_name["series_key"] == "Strategy | Side"
    assert rename_by_name["Value #A"] == "0s Avg"
    assert rename_by_name["Value #B"] == "30s Avg"
    assert rename_by_name["Value #C"] == "60s Avg"
    assert rename_by_name["Value #D"] == "120s Avg"
    assert rename_by_name["Value #E"] == "0s N"
    assert rename_by_name["Value #F"] == "30s N"
    assert rename_by_name["Value #G"] == "60s N"
    assert rename_by_name["Value #H"] == "120s N"
    assert rename_by_name["Value #I"] == "0s Res%"
    assert rename_by_name["Value #J"] == "30s Res%"
    assert rename_by_name["Value #K"] == "60s Res%"
    assert rename_by_name["Value #L"] == "120s Res%"

    assert index_by_name["series_key"] == 0
    assert index_by_name["Value #A"] == 1
    assert index_by_name["Value #B"] == 2
    assert index_by_name["Value #C"] == 3
    assert index_by_name["Value #D"] == 4
    assert index_by_name["Value #E"] == 5
    assert index_by_name["Value #F"] == 6
    assert index_by_name["Value #G"] == 7
    assert index_by_name["Value #H"] == 8
    assert index_by_name["Value #I"] == 9
    assert index_by_name["Value #J"] == 10
    assert index_by_name["Value #K"] == 11
    assert index_by_name["Value #L"] == 12

    strategy_override = next(
        override
        for override in snapshot_panel["fieldConfig"]["overrides"]
        if override["matcher"] == {"id": "byName", "options": "Strategy | Side"}
    )
    strategy_width = next(
        prop["value"] for prop in strategy_override["properties"] if prop["id"] == "custom.width"
    )
    assert strategy_width >= 360


def test_markouts_timeseries_queries_use_full_filter_scope_and_clear_legends() -> None:
    path = _repo_root() / "monitoring/grafana/dashboards/tokenmm_markouts_v1.json"
    payload = json.loads(path.read_text(encoding="utf-8"))

    timeseries_panels = [panel for panel in payload["panels"] if panel["type"] == "timeseries"]
    assert timeseries_panels

    for panel in timeseries_panels:
        for target in panel["targets"]:
            expr = target["expr"]
            legend = target["legendFormat"]
            assert "strategy_id=~\"$strategy_id\"" in expr
            assert "venue=~\"$venue\"" in expr
            assert "symbol=~\"$symbol\"" in expr
            assert "order_side=~\"$order_side\"" in expr
            assert "horizon_s=~\"$horizon_s\"" in expr
            assert "benchmark_name=~\"$benchmark_name\"" in expr
            assert "$window" in expr
            assert "{{strategy_id}}" in legend
            assert "{{venue}}" in legend
            assert "{{symbol}}" in legend
            assert "{{benchmark_name}}" in legend
            assert "{{horizon_s}}" in legend
            assert "{{order_side}}" in legend


def test_markouts_dashboard_uses_window_as_analysis_window_selector() -> None:
    path = _repo_root() / "monitoring/grafana/dashboards/tokenmm_markouts_v1.json"
    payload = json.loads(path.read_text(encoding="utf-8"))

    window_var = next(
        variable
        for variable in payload["templating"]["list"]
        if variable["name"] == "window"
    )
    assert window_var["query"] == "15m,1h,4h,24h"

    snapshot_panel = next(panel for panel in payload["panels"] if panel["id"] == 1)
    snapshot_exprs = [target["expr"] for target in snapshot_panel["targets"]]
    assert all('analysis_window=~"$window"' in expr for expr in snapshot_exprs)
    assert all("[$window]" not in expr for expr in snapshot_exprs)

    timeseries_panels = [panel for panel in payload["panels"] if panel["type"] == "timeseries"]
    assert timeseries_panels
    for panel in timeseries_panels:
        for target in panel["targets"]:
            expr = target["expr"]
            assert 'analysis_window=~"$window"' in expr
            assert "[$window]" not in expr


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

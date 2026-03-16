from __future__ import annotations

import sqlite3
from pathlib import Path

import pandas as pd
from prometheus_client import CollectorRegistry

import ops.scripts.exporters.tokenmm_markouts_exporter as markouts_exporter
from ops.scripts.exporters.tokenmm_markouts_exporter import TokenMMMarkoutsExporter
from ops.scripts.exporters.tokenmm_markouts_exporter import build_parser
from ops.scripts.exporters.tokenmm_markouts_exporter import default_db_paths
from ops.scripts.exporters.tokenmm_markouts_exporter import _resolve_paths


def _create_table(path: Path, table: str, schema: str, rows: list[dict[str, object]]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    columns = list(rows[0].keys()) if rows else []
    placeholders = ", ".join(f":{column}" for column in columns)
    with sqlite3.connect(path) as conn:
        conn.execute(schema)
        if rows:
            conn.executemany(
                f"INSERT INTO {table} ({', '.join(columns)}) VALUES ({placeholders})",
                rows,
            )
        conn.commit()


def test_default_db_paths_use_tokenmm_telemetry_dir_and_allow_override(tmp_path: Path) -> None:
    default_paths = default_db_paths(profile="tokenmm")
    assert default_paths["fills"] == Path("/var/lib/nautilus/telemetry/tokenmm/fills.sqlite")
    assert default_paths["markouts"] == Path("/var/lib/nautilus/telemetry/tokenmm/markouts.sqlite")

    override_paths = default_db_paths(profile="tokenmm", telemetry_dir=tmp_path)
    assert override_paths["fills"] == tmp_path / "fills.sqlite"
    assert override_paths["markouts"] == tmp_path / "markouts.sqlite"


def test_resolve_paths_honors_explicit_cli_db_overrides(tmp_path: Path) -> None:
    parser = build_parser()
    fills_db = tmp_path / "custom-fills.sqlite"
    markouts_db = tmp_path / "custom-markouts.sqlite"

    args = parser.parse_args(
        [
            "--profile",
            "tokenmm",
            "--telemetry-dir",
            str(tmp_path / "ignored"),
            "--fills-db",
            str(fills_db),
            "--markouts-db",
            str(markouts_db),
        ],
    )

    paths = _resolve_paths(args)

    assert paths["fills"] == fills_db
    assert paths["markouts"] == markouts_db


def test_load_markout_snapshot_bounds_markout_reads_and_matching_fill_reads(
    monkeypatch,
    tmp_path: Path,
) -> None:
    now_ms = 1_700_000_200_000
    window_hours = 1.0
    queries: list[tuple[str, str]] = []

    def _no_full_table_load(*_args: object, **_kwargs: object) -> None:
        raise AssertionError("unexpected full-table load")

    def _fake_load_sqlite_query(path: Path, query: str) -> pd.DataFrame:
        queries.append((path.name, query))
        if "FROM execution_markout" in query:
            assert "WHERE" in query
            assert "benchmark_name" in query
            assert "target_ts_ms" in query
            assert str(now_ms - int(window_hours * 60 * 60 * 1000)) in query
            assert str(now_ms) in query
            return pd.DataFrame(
                [
                    {
                        "trader_id": "TRADER-1",
                        "event_id": "fill-2",
                        "strategy_id": "plumeusdt_bybit_perp_makerv3",
                        "benchmark_name": "fv_market_mid",
                        "horizon_s": 30,
                        "target_ts_ms": now_ms - 1_000,
                        "markout_bps": "12",
                        "fill_px": "101",
                        "fill_qty": "1",
                        "resolution_status": "resolved",
                    },
                ],
            )
        if "FROM execution_fill" in query:
            assert "WHERE" in query
            assert "TRADER-1" in query
            assert "fill-2" in query
            assert "fill-1" not in query
            return pd.DataFrame(
                [
                    {
                        "trader_id": "TRADER-1",
                        "event_id": "fill-2",
                        "strategy_id": "plumeusdt_bybit_perp_makerv3",
                        "order_side": "SELL",
                        "instrument_id": "PLUMEUSDT-PERP.BYBIT",
                        "fill_px": "101",
                        "fill_qty": "1",
                        "fill_ts_ms": now_ms - 10_000,
                    },
                ],
            )
        raise AssertionError(f"unexpected query: {query}")

    monkeypatch.setattr(
        markouts_exporter,
        "load_sqlite_table",
        _no_full_table_load,
        raising=False,
    )
    monkeypatch.setattr(
        markouts_exporter,
        "load_sqlite_query",
        _fake_load_sqlite_query,
        raising=False,
    )

    rows = markouts_exporter.load_markout_snapshot(
        fills_path=tmp_path / "fills.sqlite",
        markouts_path=tmp_path / "markouts.sqlite",
        benchmark_name="fv_market_mid",
        window_hours=window_hours,
        now_ms=now_ms,
    )

    assert len(rows) == 1
    assert rows[0]["strategy_id"] == "plumeusdt_bybit_perp_makerv3"
    assert rows[0]["venue"] == "BYBIT"
    assert rows[0]["symbol"] == "PLUMEUSDT"
    assert rows[0]["order_side"] == "SELL"
    assert rows[0]["horizon_s"] == "30"
    assert rows[0]["avg_bps"] == 12.0
    assert rows[0]["fill_count"] == 1
    assert [name for name, _query in queries] == ["markouts.sqlite", "fills.sqlite"]


def test_markouts_exporter_aggregates_existing_sqlite_rows_without_new_schema_fields(
    tmp_path: Path,
) -> None:
    fills_path = tmp_path / "fills.sqlite"
    markouts_path = tmp_path / "markouts.sqlite"

    _create_table(
        fills_path,
        "execution_fill",
        """
        CREATE TABLE execution_fill (
            trader_id TEXT NOT NULL,
            event_id TEXT NOT NULL,
            strategy_id TEXT NOT NULL,
            order_side TEXT NOT NULL,
            instrument_id TEXT NOT NULL,
            fill_px TEXT NOT NULL,
            fill_qty TEXT NOT NULL,
            fill_ts_ms INTEGER NOT NULL
        )
        """,
        [
            {
                "trader_id": "TRADER-1",
                "event_id": "fill-1",
                "strategy_id": "plumeusdt_bybit_perp_makerv3",
                "order_side": "BUY",
                "instrument_id": "PLUMEUSDT-PERP.BYBIT",
                "fill_px": "100",
                "fill_qty": "2",
                "fill_ts_ms": 1_700_000_000_000,
            },
            {
                "trader_id": "TRADER-1",
                "event_id": "fill-2",
                "strategy_id": "plumeusdt_bybit_perp_makerv3",
                "order_side": "SELL",
                "instrument_id": "PLUMEUSDT-PERP.BYBIT",
                "fill_px": "101",
                "fill_qty": "1",
                "fill_ts_ms": 1_700_000_100_000,
            },
        ],
    )
    _create_table(
        markouts_path,
        "execution_markout",
        """
        CREATE TABLE execution_markout (
            trader_id TEXT NOT NULL,
            event_id TEXT NOT NULL,
            strategy_id TEXT NOT NULL,
            benchmark_name TEXT NOT NULL,
            horizon_s INTEGER NOT NULL,
            target_ts_ms INTEGER NOT NULL,
            markout_bps TEXT,
            fill_px TEXT NOT NULL,
            fill_qty TEXT NOT NULL,
            resolution_status TEXT NOT NULL
        )
        """,
        [
            {
                "trader_id": "TRADER-1",
                "event_id": "fill-1",
                "strategy_id": "plumeusdt_bybit_perp_makerv3",
                "benchmark_name": "fv_market_mid",
                "horizon_s": 30,
                "target_ts_ms": 1_700_000_030_000,
                "markout_bps": "10",
                "fill_px": "100",
                "fill_qty": "2",
                "resolution_status": "resolved",
            },
            {
                "trader_id": "TRADER-1",
                "event_id": "fill-2",
                "strategy_id": "plumeusdt_bybit_perp_makerv3",
                "benchmark_name": "fv_market_mid",
                "horizon_s": 30,
                "target_ts_ms": 1_700_000_130_000,
                "markout_bps": None,
                "fill_px": "101",
                "fill_qty": "1",
                "resolution_status": "expired",
            },
            {
                "trader_id": "TRADER-1",
                "event_id": "fill-1",
                "strategy_id": "plumeusdt_bybit_perp_makerv3",
                "benchmark_name": "fv_market_mid",
                "horizon_s": 60,
                "target_ts_ms": 1_700_000_060_000,
                "markout_bps": "20",
                "fill_px": "100",
                "fill_qty": "2",
                "resolution_status": "resolved",
            },
        ],
    )

    registry = CollectorRegistry(auto_describe=True)
    exporter = TokenMMMarkoutsExporter(
        fills_path=fills_path,
        markouts_path=markouts_path,
        env="prod",
        profile="tokenmm",
        window_hours=24.0,
        registry=registry,
    )

    exporter.poll_once(now_ms=1_700_000_200_000)

    common_labels = {
        "env": "prod",
        "profile": "tokenmm",
        "strategy_id": "plumeusdt_bybit_perp_makerv3",
        "venue": "BYBIT",
        "symbol": "PLUMEUSDT",
        "benchmark_name": "fv_market_mid",
    }

    assert registry.get_sample_value(
        "tokenmm_markout_avg_bps",
        {**common_labels, "order_side": "BUY", "horizon_s": "30"},
    ) == 10.0
    assert registry.get_sample_value(
        "tokenmm_markout_nw_bps",
        {**common_labels, "order_side": "BUY", "horizon_s": "30"},
    ) == 10.0
    assert registry.get_sample_value(
        "tokenmm_markout_resolved_rows",
        {**common_labels, "order_side": "BUY", "horizon_s": "30"},
    ) == 1.0
    assert registry.get_sample_value(
        "tokenmm_markout_fill_count",
        {**common_labels, "order_side": "BUY", "horizon_s": "30"},
    ) == 1.0
    assert registry.get_sample_value(
        "tokenmm_markout_resolution_rate",
        {**common_labels, "order_side": "BUY", "horizon_s": "30"},
    ) == 1.0
    assert registry.get_sample_value(
        "tokenmm_markout_last_target_ts_seconds",
        {**common_labels, "order_side": "BUY", "horizon_s": "60"},
    ) == 1_700_000_060.0

    sell_labels = {**common_labels, "order_side": "SELL", "horizon_s": "30"}
    assert registry.get_sample_value("tokenmm_markout_resolved_rows", sell_labels) == 0.0
    assert registry.get_sample_value("tokenmm_markout_fill_count", sell_labels) == 1.0
    assert registry.get_sample_value("tokenmm_markout_resolution_rate", sell_labels) == 0.0
    assert registry.get_sample_value("tokenmm_markout_avg_bps", sell_labels) is None
    assert registry.get_sample_value("tokenmm_markout_nw_bps", sell_labels) is None

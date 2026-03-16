from __future__ import annotations

import sqlite3
from pathlib import Path

from prometheus_client import CollectorRegistry

from ops.scripts.exporters.tokenmm_markouts_exporter import TokenMMMarkoutsExporter
from ops.scripts.exporters.tokenmm_markouts_exporter import default_db_paths


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

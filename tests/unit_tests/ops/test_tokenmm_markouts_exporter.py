from __future__ import annotations

import sqlite3
from pathlib import Path

import pandas as pd
import pytest
from prometheus_client import CollectorRegistry

import ops.scripts.exporters.tokenmm_markouts_exporter as markouts_exporter
from ops.scripts.exporters.tokenmm_markouts_exporter import TokenMMMarkoutsExporter
from ops.scripts.exporters.tokenmm_markouts_exporter import build_parser
from ops.scripts.exporters.tokenmm_markouts_exporter import default_db_paths
from ops.scripts.exporters.tokenmm_markouts_exporter import _build_fills_query
from ops.scripts.exporters.tokenmm_markouts_exporter import _build_markouts_query
from ops.scripts.exporters.tokenmm_markouts_exporter import _poll_once_with_logging
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


def test_build_fills_query_uses_compact_tuple_lookup() -> None:
    query = _build_fills_query(
        [
            {"trader_id": "TRADER-1", "event_id": "fill-2"},
            {"trader_id": "TRADER-1", "event_id": "fill-2"},
            {"trader_id": "TRADER-2", "event_id": "fill-3"},
        ],
    )

    assert query is not None
    assert "(trader_id, event_id) IN" in query
    assert " OR " not in query
    assert "SELECT trader_id, event_id, strategy_id, order_side, instrument_id, fill_px, fill_qty, fill_ts_ms" in query


def test_fill_query_columns_for_path_prefers_live_normalized_quantity_columns(tmp_path: Path) -> None:
    fills_path = tmp_path / "fills.sqlite"
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
            last_px TEXT NOT NULL,
            last_qty TEXT NOT NULL,
            last_qty_base TEXT,
            last_qty_venue TEXT,
            qty_conversion_status TEXT,
            qty_conversion_source TEXT,
            ts_event INTEGER NOT NULL
        )
        """,
        [
            {
                "trader_id": "TRADER-1",
                "event_id": "fill-1",
                "strategy_id": "plumeusdt_okx_perp_makerv3",
                "order_side": "BUY",
                "instrument_id": "PLUME-USDT-SWAP.OKX",
                "last_px": "0.012736",
                "last_qty": "100",
                "last_qty_base": "1000",
                "last_qty_venue": "100",
                "qty_conversion_status": "exact_multiplier",
                "qty_conversion_source": "generic:multiplier",
                "ts_event": 1_700_000_000_000_000_000,
            },
        ],
    )

    columns = markouts_exporter._fill_query_columns_for_path(fills_path)
    query = _build_fills_query(
        [{"trader_id": "TRADER-1", "event_id": "fill-1"}],
        select_columns=columns,
    )

    assert query is not None
    assert "COALESCE(last_qty_base, last_qty) AS fill_qty" in query
    assert "COALESCE(last_qty_venue, last_qty) AS fill_qty_venue" in query


def test_build_markouts_query_projects_only_required_columns() -> None:
    query = _build_markouts_query(
        benchmark_name="fv_market_mid",
        window_hours=24.0,
        now_ms=1_700_000_200_000,
    )

    assert "SELECT trader_id, event_id, strategy_id, benchmark_name, horizon_s, target_ts_ms, markout_bps, fill_px, fill_qty, resolution_status" in query
    assert "FROM execution_markout" in query


def test_build_parser_rejects_unbounded_or_invalid_poll_configuration() -> None:
    parser = build_parser()

    with pytest.raises(SystemExit):
        parser.parse_args(["--window-hours", "0"])
    with pytest.raises(SystemExit):
        parser.parse_args(["--window-hours", "1"])
    with pytest.raises(SystemExit):
        parser.parse_args(["--window-hours", "24"])
    with pytest.raises(SystemExit):
        parser.parse_args(["--window-hours", "167"])
    with pytest.raises(SystemExit):
        parser.parse_args(["--window-hours", "-1"])
    with pytest.raises(SystemExit):
        parser.parse_args(["--poll-interval-s", "0"])
    with pytest.raises(SystemExit):
        parser.parse_args(["--poll-interval-s", "0.4"])


def test_exporter_rejects_non_positive_window_hours(tmp_path: Path) -> None:
    with pytest.raises(ValueError):
        TokenMMMarkoutsExporter(
            fills_path=tmp_path / "fills.sqlite",
            markouts_path=tmp_path / "markouts.sqlite",
            env="prod",
            profile="tokenmm",
            window_hours=0,
        )
    with pytest.raises(ValueError):
        TokenMMMarkoutsExporter(
            fills_path=tmp_path / "fills.sqlite",
            markouts_path=tmp_path / "markouts.sqlite",
            env="prod",
            profile="tokenmm",
            window_hours=1.0,
        )
    with pytest.raises(ValueError):
        TokenMMMarkoutsExporter(
            fills_path=tmp_path / "fills.sqlite",
            markouts_path=tmp_path / "markouts.sqlite",
            env="prod",
            profile="tokenmm",
            window_hours=24.0,
        )


def test_poll_once_with_logging_catches_and_logs_poll_failures(caplog) -> None:
    class BrokenExporter:
        fills_path = Path("/tmp/fills.sqlite")
        markouts_path = Path("/tmp/markouts.sqlite")

        def poll_once(self) -> None:
            raise FileNotFoundError("fills.sqlite")

    with caplog.at_level("ERROR"):
        _poll_once_with_logging(BrokenExporter())

    assert "markouts poll failed" in caplog.text
    assert "/tmp/fills.sqlite" in caplog.text
    assert "/tmp/markouts.sqlite" in caplog.text
    assert "fills.sqlite" in caplog.text


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
        window_hours=168.0,
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
        "analysis_window": "1d",
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


def test_markouts_exporter_emits_multiple_benchmarks_from_one_process(tmp_path: Path) -> None:
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
                "fill_qty": "1",
                "fill_ts_ms": 1_700_000_000_000,
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
                "fill_qty": "1",
                "resolution_status": "resolved",
            },
            {
                "trader_id": "TRADER-1",
                "event_id": "fill-1",
                "strategy_id": "plumeusdt_bybit_perp_makerv3",
                "benchmark_name": "local_mkt_mid",
                "horizon_s": 30,
                "target_ts_ms": 1_700_000_030_000,
                "markout_bps": "4",
                "fill_px": "100",
                "fill_qty": "1",
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
        benchmark_name="fv_market_mid,local_mkt_mid",
        window_hours=168.0,
        registry=registry,
    )

    exporter.poll_once(now_ms=1_700_000_200_000)

    common_labels = {
        "env": "prod",
        "profile": "tokenmm",
        "strategy_id": "plumeusdt_bybit_perp_makerv3",
        "venue": "BYBIT",
        "symbol": "PLUMEUSDT",
        "order_side": "BUY",
        "horizon_s": "30",
        "analysis_window": "1d",
    }
    assert registry.get_sample_value(
        "tokenmm_markout_avg_bps",
        {**common_labels, "benchmark_name": "fv_market_mid"},
    ) == 10.0
    assert registry.get_sample_value(
        "tokenmm_markout_avg_bps",
        {**common_labels, "benchmark_name": "local_mkt_mid"},
    ) == 4.0


def test_markouts_exporter_emits_true_analysis_windows(tmp_path: Path) -> None:
    fills_path = tmp_path / "fills.sqlite"
    markouts_path = tmp_path / "markouts.sqlite"
    now_ms = 1_700_000_200_000

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
                "event_id": "fill-recent",
                "strategy_id": "plumeusdt_bybit_perp_makerv3",
                "order_side": "BUY",
                "instrument_id": "PLUMEUSDT-PERP.BYBIT",
                "fill_px": "100",
                "fill_qty": "1",
                "fill_ts_ms": now_ms - 11 * 60 * 1000,
            },
            {
                "trader_id": "TRADER-1",
                "event_id": "fill-old",
                "strategy_id": "plumeusdt_bybit_perp_makerv3",
                "order_side": "BUY",
                "instrument_id": "PLUMEUSDT-PERP.BYBIT",
                "fill_px": "100",
                "fill_qty": "1",
                "fill_ts_ms": now_ms - 6 * 24 * 60 * 60 * 1000,
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
                "event_id": "fill-recent",
                "strategy_id": "plumeusdt_bybit_perp_makerv3",
                "benchmark_name": "fv_market_mid",
                "horizon_s": 120,
                "target_ts_ms": now_ms - 10 * 60 * 1000,
                "markout_bps": "10",
                "fill_px": "100",
                "fill_qty": "1",
                "resolution_status": "resolved",
            },
            {
                "trader_id": "TRADER-1",
                "event_id": "fill-old",
                "strategy_id": "plumeusdt_bybit_perp_makerv3",
                "benchmark_name": "fv_market_mid",
                "horizon_s": 120,
                "target_ts_ms": now_ms - 6 * 24 * 60 * 60 * 1000,
                "markout_bps": "30",
                "fill_px": "100",
                "fill_qty": "1",
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
        benchmark_name="fv_market_mid",
        window_hours=168.0,
        registry=registry,
    )

    exporter.poll_once(now_ms=now_ms)

    assert [label for label, _hours in markouts_exporter.ANALYSIS_WINDOWS] == [
        "15m",
        "1h",
        "2h",
        "4h",
        "1d",
        "2d",
        "3d",
        "1w",
    ]

    common_labels = {
        "env": "prod",
        "profile": "tokenmm",
        "strategy_id": "plumeusdt_bybit_perp_makerv3",
        "venue": "BYBIT",
        "symbol": "PLUMEUSDT",
        "order_side": "BUY",
        "horizon_s": "120",
        "benchmark_name": "fv_market_mid",
    }

    assert registry.get_sample_value(
        "tokenmm_markout_avg_bps",
        {**common_labels, "analysis_window": "15m"},
    ) == 10.0
    assert registry.get_sample_value(
        "tokenmm_markout_avg_bps",
        {**common_labels, "analysis_window": "1d"},
    ) == 10.0
    assert registry.get_sample_value(
        "tokenmm_markout_avg_bps",
        {**common_labels, "analysis_window": "1w"},
    ) == 20.0
    assert registry.get_sample_value(
        "tokenmm_markout_fill_count",
        {**common_labels, "analysis_window": "15m"},
    ) == 1.0
    assert registry.get_sample_value(
        "tokenmm_markout_fill_count",
        {**common_labels, "analysis_window": "1d"},
    ) == 1.0
    assert registry.get_sample_value(
        "tokenmm_markout_fill_count",
        {**common_labels, "analysis_window": "1w"},
    ) == 2.0


def test_markouts_exporter_reuses_one_sqlite_read_cycle_per_poll(
    monkeypatch,
    tmp_path: Path,
) -> None:
    fills_path = tmp_path / "fills.sqlite"
    markouts_path = tmp_path / "markouts.sqlite"
    now_ms = 1_700_000_200_000

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
                "fill_qty": "1",
                "fill_ts_ms": now_ms - 11 * 60 * 1000,
            },
            {
                "trader_id": "TRADER-1",
                "event_id": "fill-2",
                "strategy_id": "plumeusdt_bybit_perp_makerv3",
                "order_side": "BUY",
                "instrument_id": "PLUMEUSDT-PERP.BYBIT",
                "fill_px": "100",
                "fill_qty": "1",
                "fill_ts_ms": now_ms - 2 * 60 * 60 * 1000,
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
                "horizon_s": 120,
                "target_ts_ms": now_ms - 10 * 60 * 1000,
                "markout_bps": "10",
                "fill_px": "100",
                "fill_qty": "1",
                "resolution_status": "resolved",
            },
            {
                "trader_id": "TRADER-1",
                "event_id": "fill-2",
                "strategy_id": "plumeusdt_bybit_perp_makerv3",
                "benchmark_name": "fv_market_mid",
                "horizon_s": 120,
                "target_ts_ms": now_ms - 2 * 60 * 60 * 1000,
                "markout_bps": "30",
                "fill_px": "100",
                "fill_qty": "1",
                "resolution_status": "resolved",
            },
        ],
    )

    original_load_sqlite_query = markouts_exporter.load_sqlite_query
    query_counts = {"markouts": 0, "fills": 0}

    def _counting_load_sqlite_query(path: Path, query: str) -> pd.DataFrame:
        if "FROM execution_markout" in query:
            query_counts["markouts"] += 1
        if "FROM execution_fill" in query:
            query_counts["fills"] += 1
        return original_load_sqlite_query(path, query)

    monkeypatch.setattr(
        markouts_exporter,
        "load_sqlite_query",
        _counting_load_sqlite_query,
        raising=False,
    )

    exporter = TokenMMMarkoutsExporter(
        fills_path=fills_path,
        markouts_path=markouts_path,
        env="prod",
        profile="tokenmm",
        benchmark_name="fv_market_mid",
        window_hours=168.0,
        registry=CollectorRegistry(auto_describe=True),
    )

    exporter.poll_once(now_ms=now_ms)

    assert query_counts == {"markouts": 1, "fills": 1}


def test_markouts_exporter_uses_configured_window_hours_for_bounded_read(
    monkeypatch,
    tmp_path: Path,
) -> None:
    captured_window_hours: list[float] = []

    def _fake_load_merged_markout_dataset(**kwargs: object) -> pd.DataFrame:
        captured_window_hours.append(float(kwargs["window_hours"]))
        return pd.DataFrame()

    monkeypatch.setattr(
        markouts_exporter,
        "_load_merged_markout_dataset",
        _fake_load_merged_markout_dataset,
        raising=False,
    )

    exporter = TokenMMMarkoutsExporter(
        fills_path=tmp_path / "fills.sqlite",
        markouts_path=tmp_path / "markouts.sqlite",
        env="prod",
        profile="tokenmm",
        benchmark_name="fv_market_mid",
        window_hours=168.0,
        registry=CollectorRegistry(auto_describe=True),
    )

    exporter.poll_once(now_ms=1_700_000_200_000)

    assert captured_window_hours == [168.0]


def test_load_markout_snapshot_supports_live_fill_schema_columns(tmp_path: Path) -> None:
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
            last_px TEXT NOT NULL,
            last_qty TEXT NOT NULL,
            ts_event INTEGER NOT NULL
        )
        """,
        [
            {
                "trader_id": "TRADER-1",
                "event_id": "fill-1",
                "strategy_id": "plumeusdt_bybit_spot_makerv3",
                "order_side": "BUY",
                "instrument_id": "PLUMEUSDT-SPOT.BYBIT",
                "last_px": "0.01325",
                "last_qty": "1000",
                "ts_event": 1_700_000_000_000_000_000,
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
                "strategy_id": "plumeusdt_bybit_spot_makerv3",
                "benchmark_name": "fv_market_mid",
                "horizon_s": 30,
                "target_ts_ms": 1_700_000_000_030,
                "markout_bps": "8.5",
                "fill_px": "0.01325",
                "fill_qty": "1000",
                "resolution_status": "resolved",
            },
        ],
    )

    rows = markouts_exporter.load_markout_snapshot(
        fills_path=fills_path,
        markouts_path=markouts_path,
        benchmark_name="fv_market_mid",
        window_hours=24.0,
        now_ms=1_700_000_000_100,
    )

    assert len(rows) == 1
    assert rows[0]["strategy_id"] == "plumeusdt_bybit_spot_makerv3"
    assert rows[0]["venue"] == "BYBIT"
    assert rows[0]["symbol"] == "PLUMEUSDT"
    assert rows[0]["order_side"] == "BUY"
    assert rows[0]["fill_count"] == 1
    assert rows[0]["avg_bps"] == 8.5


def test_live_fill_query_prefers_base_quantity_columns_when_available(tmp_path: Path) -> None:
    fills_path = tmp_path / "fills.sqlite"

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
            last_px TEXT NOT NULL,
            last_qty TEXT NOT NULL,
            last_qty_base TEXT,
            last_qty_venue TEXT,
            ts_event INTEGER NOT NULL
        )
        """,
        [
            {
                "trader_id": "TRADER-1",
                "event_id": "fill-1",
                "strategy_id": "plumeusdt_okx_perp_makerv3",
                "order_side": "BUY",
                "instrument_id": "PLUME-USDT-SWAP.OKX",
                "last_px": "0.012736",
                "last_qty": "100",
                "last_qty_base": "1000",
                "last_qty_venue": "100",
                "ts_event": 1_700_000_000_000_000_000,
            },
        ],
    )

    query = _build_fills_query(
        [{"trader_id": "TRADER-1", "event_id": "fill-1"}],
        select_columns=markouts_exporter._fill_query_columns_for_path(fills_path),
    )

    assert query is not None
    assert "COALESCE(last_qty_base, last_qty) AS fill_qty" in query
    assert "COALESCE(last_qty_venue, last_qty) AS fill_qty_venue" in query
    assert "last_qty_base AS fill_qty_base" in query

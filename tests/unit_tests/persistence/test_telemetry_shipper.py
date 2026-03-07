from __future__ import annotations

import sqlite3
from pathlib import Path

from nautilus_trader.persistence.shipper.config import TelemetryPostgresConfig
from nautilus_trader.persistence.shipper.config import TelemetryShipperConfig
from nautilus_trader.persistence.shipper.postgres import TABLE_CREATE_SQL
from nautilus_trader.persistence.shipper.postgres import TABLE_PRIMARY_KEYS
from nautilus_trader.persistence.shipper.service import SQLiteToPostgresTelemetryShipper


class _RecordingSink:
    def __init__(self) -> None:
        self.insert_calls: list[tuple[str, list[dict[str, object]]]] = []

    def ensure_schema(self) -> None:
        return None

    def insert_rows(self, table_name: str, rows: list[dict[str, object]]) -> int:
        self.insert_calls.append((table_name, rows))
        return len(rows)


def _create_quote_cycle_source(path: Path) -> None:
    conn = sqlite3.connect(path)
    try:
        conn.execute(
            """
            CREATE TABLE quote_cycle (
              trader_id TEXT NOT NULL,
              strategy_id TEXT NOT NULL,
              instrument_id TEXT NOT NULL,
              run_id TEXT NOT NULL,
              quote_cycle_id TEXT NOT NULL,
              quote_cycle_seq INTEGER NOT NULL,
              quote_cycle_event TEXT NOT NULL,
              reason_code TEXT NOT NULL,
              trigger_source TEXT,
              trigger_instrument_id TEXT,
              trigger_md_ts_event_ns INTEGER,
              trigger_md_ts_init_ns INTEGER,
              ts_cycle_start_ns INTEGER,
              ts_cycle_end_ns INTEGER,
              state_from TEXT,
              state_to TEXT,
              cancel_count INTEGER,
              place_count INTEGER,
              bid_levels INTEGER,
              ask_levels INTEGER,
              decision_context_json TEXT NOT NULL DEFAULT 'null',
              created_at TEXT NOT NULL
            )
            """,
        )
        conn.executemany(
            """
            INSERT INTO quote_cycle (
              trader_id,
              strategy_id,
              instrument_id,
              run_id,
              quote_cycle_id,
              quote_cycle_seq,
              quote_cycle_event,
              reason_code,
              decision_context_json,
              created_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            """,
            [
                (
                    "TRADER-001",
                    "strategy-a",
                    "PLUMEUSDT.BINANCE_SPOT",
                    "run-1",
                    "run-1:1",
                    1,
                    "completed",
                    "completed_rebalanced",
                    '{"edge_bps": 2.5}',
                    "2025-01-01T00:00:00.000Z",
                ),
                (
                    "TRADER-001",
                    "strategy-a",
                    "PLUMEUSDT.BINANCE_SPOT",
                    "run-1",
                    "run-1:2",
                    2,
                    "completed",
                    "completed_no_actions",
                    "null",
                    "2099-01-01T00:00:00.000Z",
                ),
            ],
        )
        conn.commit()
    finally:
        conn.close()


def _create_balance_snapshot_source(path: Path) -> None:
    conn = sqlite3.connect(path)
    try:
        conn.execute(
            """
            CREATE TABLE flux_balance_snapshot (
              trader_id TEXT NOT NULL,
              strategy_id TEXT NOT NULL,
              snapshot_id TEXT NOT NULL,
              topic TEXT NOT NULL,
              snapshot_hash TEXT NOT NULL,
              ts_event_ns INTEGER,
              ts_ms INTEGER NOT NULL,
              ts_ingest_ns INTEGER NOT NULL,
              account_count INTEGER NOT NULL,
              position_count INTEGER NOT NULL,
              payload_json TEXT NOT NULL,
              created_at TEXT NOT NULL
            )
            """,
        )
        conn.execute(
            """
            CREATE TABLE flux_balance_snapshot_row (
              trader_id TEXT NOT NULL,
              strategy_id TEXT NOT NULL,
              snapshot_id TEXT NOT NULL,
              row_key TEXT NOT NULL,
              kind TEXT NOT NULL,
              exchange TEXT,
              account_id TEXT,
              account TEXT,
              asset TEXT,
              instrument_id TEXT,
              side TEXT,
              signed_qty TEXT,
              quantity TEXT,
              free TEXT,
              locked TEXT,
              total TEXT,
              avg_px_open TEXT,
              avg_px_close TEXT,
              realized_pnl TEXT,
              ts_ms INTEGER NOT NULL,
              row_json TEXT NOT NULL,
              created_at TEXT NOT NULL
            )
            """,
        )
        conn.execute(
            """
            INSERT INTO flux_balance_snapshot (
              trader_id, strategy_id, snapshot_id, topic, snapshot_hash, ts_event_ns, ts_ms,
              ts_ingest_ns, account_count, position_count, payload_json, created_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            """,
            (
                "TRADER-001",
                "maker_v3_01",
                "snapshot-1",
                "flux.makerv3.balances",
                "hash-1",
                123_000_000_000,
                123_000,
                124_000_000_000,
                1,
                1,
                '{"strategy_id":"maker_v3_01"}',
                "2099-01-01T00:00:00.000Z",
            ),
        )
        conn.execute(
            """
            INSERT INTO flux_balance_snapshot_row (
              trader_id, strategy_id, snapshot_id, row_key, kind, exchange, account_id, account,
              asset, instrument_id, side, signed_qty, quantity, free, locked, total,
              avg_px_open, avg_px_close, realized_pnl, ts_ms, row_json, created_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            """,
            (
                "TRADER-001",
                "maker_v3_01",
                "snapshot-1",
                "bybit:BYBIT-001:PLUME",
                "cash",
                "bybit",
                "BYBIT-001",
                "bybit-001",
                "PLUME",
                None,
                None,
                None,
                None,
                "90",
                "10",
                "100",
                None,
                None,
                None,
                123_000,
                '{"asset":"PLUME"}',
                "2099-01-01T00:00:00.000Z",
            ),
        )
        conn.commit()
    finally:
        conn.close()


def _create_portfolio_inventory_snapshot_source(path: Path) -> None:
    conn = sqlite3.connect(path)
    try:
        conn.execute(
            """
            CREATE TABLE portfolio_inventory_snapshot (
              portfolio_id TEXT NOT NULL,
              base_currency TEXT NOT NULL,
              snapshot_id TEXT NOT NULL,
              snapshot_hash TEXT NOT NULL,
              global_qty TEXT,
              degraded INTEGER NOT NULL DEFAULT 0,
              missing_required_json TEXT NOT NULL DEFAULT '[]',
              components_json TEXT NOT NULL DEFAULT '[]',
              ts_ms INTEGER NOT NULL,
              ts_ingest_ns INTEGER NOT NULL,
              created_at TEXT NOT NULL
            )
            """,
        )
        conn.execute(
            """
            INSERT INTO portfolio_inventory_snapshot (
              portfolio_id, base_currency, snapshot_id, snapshot_hash, global_qty, degraded,
              missing_required_json, components_json, ts_ms, ts_ingest_ns, created_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            """,
            (
                "tokenmm",
                "PLUME",
                "portfolio-snapshot-1",
                "portfolio-hash-1",
                "26883",
                0,
                "[]",
                '[{"strategy_id":"maker_v3_01","local_qty":"26883"}]',
                123_000,
                124_000_000_000,
                "2099-01-01T00:00:00.000Z",
            ),
        )
        conn.commit()
    finally:
        conn.close()


def _create_balance_snapshot_source(path: Path) -> None:
    conn = sqlite3.connect(path)
    try:
        conn.executescript(
            """
            CREATE TABLE flux_balance_snapshot (
              trader_id TEXT NOT NULL,
              strategy_id TEXT NOT NULL,
              snapshot_id TEXT NOT NULL,
              topic TEXT NOT NULL,
              snapshot_hash TEXT NOT NULL,
              ts_event_ns INTEGER,
              ts_ms INTEGER NOT NULL,
              ts_ingest_ns INTEGER NOT NULL,
              account_count INTEGER NOT NULL,
              position_count INTEGER NOT NULL,
              payload_json TEXT NOT NULL,
              created_at TEXT NOT NULL
            );
            CREATE TABLE flux_balance_snapshot_row (
              trader_id TEXT NOT NULL,
              strategy_id TEXT NOT NULL,
              snapshot_id TEXT NOT NULL,
              row_key TEXT NOT NULL,
              kind TEXT NOT NULL,
              exchange TEXT,
              account_id TEXT,
              account TEXT,
              asset TEXT,
              instrument_id TEXT,
              side TEXT,
              signed_qty TEXT,
              quantity TEXT,
              free TEXT,
              locked TEXT,
              total TEXT,
              avg_px_open TEXT,
              avg_px_close TEXT,
              realized_pnl TEXT,
              ts_ms INTEGER NOT NULL,
              row_json TEXT NOT NULL,
              created_at TEXT NOT NULL
            );
            """,
        )
        conn.execute(
            """
            INSERT INTO flux_balance_snapshot (
              trader_id, strategy_id, snapshot_id, topic, snapshot_hash, ts_event_ns, ts_ms,
              ts_ingest_ns, account_count, position_count, payload_json, created_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            """,
            (
                "TRADER-001",
                "maker_v3_01",
                "snapshot-1",
                "flux.makerv3.balances",
                "hash-1",
                1_000_000,
                1_000,
                1_100_000,
                1,
                1,
                '{"strategy_id":"maker_v3_01"}',
                "2025-01-01T00:00:00.000Z",
            ),
        )
        conn.execute(
            """
            INSERT INTO flux_balance_snapshot_row (
              trader_id, strategy_id, snapshot_id, row_key, kind, exchange, account_id, account,
              asset, instrument_id, side, signed_qty, quantity, free, locked, total, avg_px_open,
              avg_px_close, realized_pnl, ts_ms, row_json, created_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            """,
            (
                "TRADER-001",
                "maker_v3_01",
                "snapshot-1",
                "bybit:BYBIT-001:PLUME",
                "cash",
                "bybit",
                "BYBIT-001",
                "bybit-001",
                "PLUME",
                None,
                None,
                None,
                None,
                "90",
                "10",
                "100",
                None,
                None,
                None,
                1_000,
                '{"asset":"PLUME"}',
                "2025-01-01T00:00:00.000Z",
            ),
        )
        conn.commit()
    finally:
        conn.close()


def _create_portfolio_inventory_source(path: Path) -> None:
    conn = sqlite3.connect(path)
    try:
        conn.execute(
            """
            CREATE TABLE portfolio_inventory_snapshot (
              portfolio_id TEXT NOT NULL,
              base_currency TEXT NOT NULL,
              snapshot_id TEXT NOT NULL,
              snapshot_hash TEXT NOT NULL,
              global_qty TEXT,
              degraded INTEGER NOT NULL,
              missing_required_json TEXT NOT NULL,
              components_json TEXT NOT NULL,
              ts_ms INTEGER NOT NULL,
              ts_ingest_ns INTEGER NOT NULL,
              created_at TEXT NOT NULL
            )
            """,
        )
        conn.execute(
            """
            INSERT INTO portfolio_inventory_snapshot (
              portfolio_id, base_currency, snapshot_id, snapshot_hash, global_qty, degraded,
              missing_required_json, components_json, ts_ms, ts_ingest_ns, created_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            """,
            (
                "tokenmm",
                "PLUME",
                "portfolio-snapshot-1",
                "portfolio-hash-1",
                "26883",
                0,
                "[]",
                '[{"strategy_id":"maker_v3_01","local_qty":"26883"}]',
                1_000,
                1_100_000,
                "2025-01-01T00:00:00.000Z",
            ),
        )
        conn.commit()
    finally:
        conn.close()


def test_shipper_ships_once_and_resumes_from_cursor(tmp_path: Path) -> None:
    quote_cycles_db = tmp_path / "quote_cycles.sqlite"
    state_db = tmp_path / "shipper_state.sqlite"
    _create_quote_cycle_source(quote_cycles_db)

    config = TelemetryShipperConfig(
        enabled=True,
        enable_local_persistence=True,
        source_profile="tokenmm",
        fills_db_path=None,
        orders_db_path=None,
        quote_cycles_db_path=str(quote_cycles_db),
        state_db_path=str(state_db),
        poll_interval_ms=1000,
        max_batch_size=100,
        prune_retention_hours=168,
        postgres=TelemetryPostgresConfig(
            host="localhost",
            port=5432,
            database="nautilus_telemetry",
            schema="telemetry",
            username="nautilus",
            password="pass",
            sslmode="require",
        ),
    )
    sink = _RecordingSink()
    shipper = SQLiteToPostgresTelemetryShipper(config=config, sink=sink, source_host="host-a")

    first = shipper.ship_once()
    second = shipper.ship_once()

    assert first["quote_cycle"].shipped == 2
    assert second["quote_cycle"].shipped == 0
    assert len(sink.insert_calls) == 1
    assert sink.insert_calls[0][0] == "quote_cycle"


def test_shipper_prunes_only_old_rows_after_success(tmp_path: Path) -> None:
    quote_cycles_db = tmp_path / "quote_cycles.sqlite"
    state_db = tmp_path / "shipper_state.sqlite"
    _create_quote_cycle_source(quote_cycles_db)

    config = TelemetryShipperConfig(
        enabled=True,
        enable_local_persistence=True,
        source_profile="tokenmm",
        fills_db_path=None,
        orders_db_path=None,
        quote_cycles_db_path=str(quote_cycles_db),
        state_db_path=str(state_db),
        poll_interval_ms=1000,
        max_batch_size=100,
        prune_retention_hours=24,
        postgres=TelemetryPostgresConfig(
            host="localhost",
            port=5432,
            database="nautilus_telemetry",
            schema="telemetry",
            username="nautilus",
            password="pass",
            sslmode="require",
        ),
    )
    shipper = SQLiteToPostgresTelemetryShipper(
        config=config,
        sink=_RecordingSink(),
        source_host="host-a",
    )

    shipper.ship_once()

    conn = sqlite3.connect(quote_cycles_db)
    try:
        rows = conn.execute(
            "SELECT quote_cycle_id FROM quote_cycle ORDER BY quote_cycle_seq ASC",
        ).fetchall()
    finally:
        conn.close()

    assert rows == [("run-1:2",)]


def test_shipper_ships_balance_snapshot_and_portfolio_inventory_tables(tmp_path: Path) -> None:
    balance_db = tmp_path / "balance_snapshots.sqlite"
    portfolio_db = tmp_path / "portfolio_inventory.sqlite"
    state_db = tmp_path / "shipper_state.sqlite"
    _create_balance_snapshot_source(balance_db)
    _create_portfolio_inventory_snapshot_source(portfolio_db)

    config = TelemetryShipperConfig(
        enabled=True,
        enable_local_persistence=True,
        source_profile="tokenmm",
        balance_snapshots_db_path=str(balance_db),
        fills_db_path=None,
        orders_db_path=None,
        quote_cycles_db_path=None,
        portfolio_inventory_db_path=str(portfolio_db),
        state_db_path=str(state_db),
        poll_interval_ms=1000,
        max_batch_size=100,
        prune_retention_hours=168,
        postgres=TelemetryPostgresConfig(
            host="localhost",
            port=5432,
            database="nautilus_telemetry",
            schema="telemetry",
            username="nautilus",
            password="pass",
            sslmode="require",
        ),
    )
    sink = _RecordingSink()
    shipper = SQLiteToPostgresTelemetryShipper(config=config, sink=sink, source_host="host-a")

    result = shipper.ship_once()

    assert result["flux_balance_snapshot"].shipped == 1
    assert result["flux_balance_snapshot_row"].shipped == 1
    assert result["portfolio_inventory_snapshot"].shipped == 1
    assert [name for name, _rows in sink.insert_calls] == [
        "flux_balance_snapshot",
        "flux_balance_snapshot_row",
        "portfolio_inventory_snapshot",
    ]


def test_shipper_resets_cursor_when_rowid_restarts_after_source_table_reuse(tmp_path: Path) -> None:
    portfolio_db = tmp_path / "portfolio_inventory.sqlite"
    state_db = tmp_path / "shipper_state.sqlite"
    _create_portfolio_inventory_snapshot_source(portfolio_db)

    config = TelemetryShipperConfig(
        enabled=True,
        enable_local_persistence=True,
        source_profile="tokenmm",
        balance_snapshots_db_path=None,
        fills_db_path=None,
        orders_db_path=None,
        quote_cycles_db_path=None,
        portfolio_inventory_db_path=str(portfolio_db),
        state_db_path=str(state_db),
        poll_interval_ms=1000,
        max_batch_size=100,
        prune_retention_hours=168,
        postgres=TelemetryPostgresConfig(
            host="localhost",
            port=5432,
            database="nautilus_telemetry",
            schema="telemetry",
            username="nautilus",
            password="pass",
            sslmode="require",
        ),
    )
    sink = _RecordingSink()
    shipper = SQLiteToPostgresTelemetryShipper(config=config, sink=sink, source_host="host-a")

    first = shipper.ship_once()
    assert first["portfolio_inventory_snapshot"].shipped == 1

    conn = sqlite3.connect(portfolio_db)
    try:
        conn.execute("DELETE FROM portfolio_inventory_snapshot")
        conn.execute(
            """
            INSERT INTO portfolio_inventory_snapshot (
              portfolio_id, base_currency, snapshot_id, snapshot_hash, global_qty, degraded,
              missing_required_json, components_json, ts_ms, ts_ingest_ns, created_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            """,
            (
                "tokenmm",
                "PLUME",
                "portfolio-snapshot-2",
                "portfolio-hash-2",
                "30000",
                0,
                "[]",
                '[{"strategy_id":"maker_v3_01","local_qty":"30000"}]',
                124_000,
                125_000_000_000,
                "2099-01-01T00:00:00.000Z",
            ),
        )
        conn.commit()
    finally:
        conn.close()

    second = shipper.ship_once()
    assert second["portfolio_inventory_snapshot"].shipped == 1


def test_postgres_sink_keys_include_source_profile_for_shared_database_isolation() -> None:
    assert TABLE_PRIMARY_KEYS["execution_fill"] == ("source_profile", "trader_id", "event_id")
    assert TABLE_PRIMARY_KEYS["order_action"] == ("source_profile", "trader_id", "event_id")
    assert TABLE_PRIMARY_KEYS["quote_cycle"] == ("source_profile", "trader_id", "quote_cycle_id")
    assert TABLE_PRIMARY_KEYS["flux_balance_snapshot"] == (
        "source_profile",
        "trader_id",
        "snapshot_id",
    )
    assert TABLE_PRIMARY_KEYS["flux_balance_snapshot_row"] == (
        "source_profile",
        "trader_id",
        "snapshot_id",
        "row_key",
    )
    assert TABLE_PRIMARY_KEYS["portfolio_inventory_snapshot"] == (
        "source_profile",
        "portfolio_id",
        "base_currency",
        "snapshot_id",
    )
    assert "PRIMARY KEY (source_profile, trader_id, event_id)" in TABLE_CREATE_SQL["execution_fill"]
    assert "PRIMARY KEY (source_profile, trader_id, event_id)" in TABLE_CREATE_SQL["order_action"]
    assert "PRIMARY KEY (source_profile, trader_id, quote_cycle_id)" in TABLE_CREATE_SQL["quote_cycle"]
    assert "PRIMARY KEY (source_profile, trader_id, snapshot_id)" in TABLE_CREATE_SQL["flux_balance_snapshot"]
    assert "PRIMARY KEY (source_profile, trader_id, snapshot_id, row_key)" in TABLE_CREATE_SQL["flux_balance_snapshot_row"]
    assert "PRIMARY KEY (source_profile, portfolio_id, base_currency, snapshot_id)" in TABLE_CREATE_SQL["portfolio_inventory_snapshot"]


def test_shipper_ships_balance_and_inventory_snapshot_tables(tmp_path: Path) -> None:
    balance_db = tmp_path / "balance_snapshots.sqlite"
    portfolio_db = tmp_path / "portfolio_inventory.sqlite"
    state_db = tmp_path / "shipper_state.sqlite"
    _create_balance_snapshot_source(balance_db)
    _create_portfolio_inventory_source(portfolio_db)

    config = TelemetryShipperConfig(
        enabled=True,
        enable_local_persistence=True,
        source_profile="tokenmm",
        balance_snapshots_db_path=str(balance_db),
        fills_db_path=None,
        orders_db_path=None,
        quote_cycles_db_path=None,
        portfolio_inventory_db_path=str(portfolio_db),
        state_db_path=str(state_db),
        poll_interval_ms=1_000,
        max_batch_size=100,
        prune_retention_hours=168,
        postgres=TelemetryPostgresConfig(
            host="localhost",
            port=5432,
            database="nautilus_telemetry",
            schema="telemetry",
            username="nautilus",
            password="pass",
            sslmode="require",
        ),
    )
    sink = _RecordingSink()
    shipper = SQLiteToPostgresTelemetryShipper(config=config, sink=sink, source_host="host-a")

    result = shipper.ship_once()

    assert result["flux_balance_snapshot"].shipped == 1
    assert result["flux_balance_snapshot_row"].shipped == 1
    assert result["portfolio_inventory_snapshot"].shipped == 1
    assert {call[0] for call in sink.insert_calls} == {
        "flux_balance_snapshot",
        "flux_balance_snapshot_row",
        "portfolio_inventory_snapshot",
    }


def test_shipper_resets_cursor_when_source_rowids_restart_after_full_prune(tmp_path: Path) -> None:
    quote_cycles_db = tmp_path / "quote_cycles.sqlite"
    state_db = tmp_path / "shipper_state.sqlite"
    _create_quote_cycle_source(quote_cycles_db)

    config = TelemetryShipperConfig(
        enabled=True,
        enable_local_persistence=True,
        source_profile="tokenmm",
        fills_db_path=None,
        orders_db_path=None,
        quote_cycles_db_path=str(quote_cycles_db),
        state_db_path=str(state_db),
        poll_interval_ms=1_000,
        max_batch_size=100,
        prune_retention_hours=24,
        postgres=TelemetryPostgresConfig(
            host="localhost",
            port=5432,
            database="nautilus_telemetry",
            schema="telemetry",
            username="nautilus",
            password="pass",
            sslmode="require",
        ),
    )
    sink = _RecordingSink()
    shipper = SQLiteToPostgresTelemetryShipper(config=config, sink=sink, source_host="host-a")
    shipper.ship_once()

    conn = sqlite3.connect(quote_cycles_db)
    try:
        conn.execute("DELETE FROM quote_cycle")
        conn.execute(
            """
            INSERT INTO quote_cycle (
              trader_id, strategy_id, instrument_id, run_id, quote_cycle_id, quote_cycle_seq,
              quote_cycle_event, reason_code, decision_context_json, created_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            """,
            (
                "TRADER-001",
                "strategy-a",
                "PLUMEUSDT.BINANCE_SPOT",
                "run-2",
                "run-2:1",
                1,
                "completed",
                "completed_rebalanced",
                '{"edge_bps": 3.0}',
                "2099-01-02T00:00:00.000Z",
            ),
        )
        conn.commit()
    finally:
        conn.close()

    result = shipper.ship_once()

    assert result["quote_cycle"].shipped == 1
    assert sink.insert_calls[-1][0] == "quote_cycle"
    assert sink.insert_calls[-1][1][0]["quote_cycle_id"] == "run-2:1"

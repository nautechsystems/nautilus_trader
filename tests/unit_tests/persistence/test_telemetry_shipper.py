from __future__ import annotations

import sys
import sqlite3
from pathlib import Path
from types import SimpleNamespace

import pytest

from nautilus_trader.persistence.fills.sqlite import connect as connect_fills
from nautilus_trader.persistence.fills.sqlite import ensure_schema as ensure_fill_schema
from nautilus_trader.persistence.fills.sqlite import fill_to_row
from nautilus_trader.persistence.fills.sqlite import insert_fills
from nautilus_trader.persistence.orders.actor import order_event_to_row
from nautilus_trader.persistence.orders.sqlite import connect as connect_orders
from nautilus_trader.persistence.orders.sqlite import ensure_schema as ensure_order_schema
from nautilus_trader.persistence.orders.sqlite import insert_many
from nautilus_trader.persistence.shipper.config import TelemetryPostgresConfig
from nautilus_trader.persistence.shipper.config import TelemetryShipperConfig
from nautilus_trader.persistence.shipper.postgres import TelemetryPostgresSink
from nautilus_trader.persistence.shipper.postgres import TABLE_CREATE_SQL
from nautilus_trader.persistence.shipper.postgres import TABLE_PRIMARY_KEYS
from nautilus_trader.persistence.shipper import run as shipper_run
from nautilus_trader.persistence.shipper.service import SQLiteToPostgresTelemetryShipper
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.events import TestEventStubs
from nautilus_trader.test_kit.stubs.execution import TestExecStubs


class _RecordingSink:
    def __init__(self) -> None:
        self.insert_calls: list[tuple[str, list[dict[str, object]]]] = []

    def ensure_schema(self) -> None:
        return None

    def insert_rows(self, table_name: str, rows: list[dict[str, object]]) -> int:
        self.insert_calls.append((table_name, rows))
        return len(rows)


class _FakeCursor:
    def __init__(self, executed: list[str]) -> None:
        self._executed = executed
        self.rowcount = 0

    def __enter__(self) -> _FakeCursor:
        return self

    def __exit__(self, exc_type, exc, tb) -> None:
        return None

    def execute(self, query, params=None) -> None:
        self._executed.append(str(query))

    def executemany(self, query, params) -> None:
        self._executed.append(str(query))
        self.rowcount = len(list(params))


class _FakeConnection:
    def __init__(self) -> None:
        self.executed: list[str] = []
        self.closed = False
        self.commits = 0

    def cursor(self) -> _FakeCursor:
        return _FakeCursor(self.executed)

    def commit(self) -> None:
        self.commits += 1

    def close(self) -> None:
        self.closed = True


class _FakeComposable(str):
    def format(self, *args) -> _FakeComposable:
        return _FakeComposable(super().format(*(str(arg) for arg in args)))


class _FakeSQLModule:
    @staticmethod
    def SQL(text: str) -> _FakeComposable:
        return _FakeComposable(text)

    @staticmethod
    def Identifier(name: str) -> str:
        return name

    @staticmethod
    def Placeholder() -> str:
        return "%s"


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


def _create_execution_fill_source(path: Path) -> None:
    conn = connect_fills(str(path))
    try:
        ensure_fill_schema(conn)
        instrument = TestInstrumentProvider.btcusdt_binance()
        order = TestExecStubs.make_accepted_order(instrument=instrument)
        fill = TestEventStubs.order_filled(order=order, instrument=instrument, ts_event=101)
        row = fill_to_row(
            fill,
            last_qty_base="100",
            last_qty_venue="100",
            qty_conversion_status="identity",
            qty_conversion_source="generic:multiplier=1",
        )
        insert_fills(conn, [row])
    finally:
        conn.close()


def _create_order_action_source(path: Path) -> None:
    conn = connect_orders(str(path))
    try:
        ensure_order_schema(conn)
        instrument = TestInstrumentProvider.ethusdt_perp_binance()
        order = TestExecStubs.limit_order(instrument=instrument)
        initialized_dict = order.init_event.to_dict(order.init_event)
        initialized_dict["order_qty_base"] = "100"
        initialized_dict["order_qty_venue"] = "100"
        initialized_dict["qty_conversion_status"] = "identity"
        initialized_dict["qty_conversion_source"] = "generic:multiplier=1"
        row = order_event_to_row(
            initialized_dict,
            event_type="OrderInitialized",
            ts_ingest=123,
        )
        assert row is not None
        insert_many(conn, [row])
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
              global_qty_base TEXT,
              global_qty TEXT,
              aggregation_mode TEXT NOT NULL DEFAULT 'strict',
              global_qty_base_complete INTEGER NOT NULL DEFAULT 0,
              global_qty_complete INTEGER NOT NULL DEFAULT 0,
              degraded INTEGER NOT NULL DEFAULT 0,
              missing_required_json TEXT NOT NULL DEFAULT '[]',
              stale_required_json TEXT NOT NULL DEFAULT '[]',
              null_qty_required_json TEXT NOT NULL DEFAULT '[]',
              components_json TEXT NOT NULL DEFAULT '[]',
              usable_component_count INTEGER NOT NULL DEFAULT 0,
              expected_component_count INTEGER NOT NULL DEFAULT 0,
              stale_after_ms INTEGER NOT NULL DEFAULT 0,
              ts_ms INTEGER NOT NULL,
              ts_ingest_ns INTEGER NOT NULL,
              created_at TEXT NOT NULL
            )
            """,
        )
        conn.execute(
            """
            INSERT INTO portfolio_inventory_snapshot (
              portfolio_id, base_currency, snapshot_id, snapshot_hash, global_qty_base, global_qty,
              aggregation_mode, global_qty_base_complete, global_qty_complete, degraded,
              missing_required_json, stale_required_json, null_qty_required_json, components_json,
              usable_component_count, expected_component_count, stale_after_ms, ts_ms, ts_ingest_ns, created_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            """,
            (
                "tokenmm",
                "PLUME",
                "portfolio-snapshot-1",
                "portfolio-hash-1",
                "26883",
                "26883",
                "partial",
                0,
                0,
                0,
                "[]",
                '["maker_v3_02"]',
                "[]",
                '[{"strategy_id":"maker_v3_01","local_qty":"26883"}]',
                1,
                2,
                3000,
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
              global_qty_base TEXT,
              global_qty TEXT,
              aggregation_mode TEXT NOT NULL DEFAULT 'strict',
              global_qty_base_complete INTEGER NOT NULL DEFAULT 0,
              global_qty_complete INTEGER NOT NULL DEFAULT 0,
              degraded INTEGER NOT NULL,
              missing_required_json TEXT NOT NULL,
              stale_required_json TEXT NOT NULL,
              null_qty_required_json TEXT NOT NULL,
              components_json TEXT NOT NULL,
              usable_component_count INTEGER NOT NULL,
              expected_component_count INTEGER NOT NULL,
              stale_after_ms INTEGER NOT NULL,
              ts_ms INTEGER NOT NULL,
              ts_ingest_ns INTEGER NOT NULL,
              created_at TEXT NOT NULL
            )
            """,
        )
        conn.execute(
            """
            INSERT INTO portfolio_inventory_snapshot (
              portfolio_id, base_currency, snapshot_id, snapshot_hash, global_qty_base, global_qty,
              aggregation_mode, global_qty_base_complete, global_qty_complete, degraded,
              missing_required_json, stale_required_json, null_qty_required_json, components_json,
              usable_component_count, expected_component_count, stale_after_ms, ts_ms, ts_ingest_ns, created_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            """,
            (
                "tokenmm",
                "PLUME",
                "portfolio-snapshot-1",
                "portfolio-hash-1",
                "26883",
                "26883",
                "partial",
                0,
                0,
                0,
                "[]",
                '["maker_v3_02"]',
                "[]",
                '[{"strategy_id":"maker_v3_01","local_qty":"26883"}]',
                1,
                2,
                3000,
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


def test_shipper_throttles_source_prune_writes_between_close_cycles(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
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

    monotonic_now = 1000.0
    monkeypatch.setattr(
        "nautilus_trader.persistence.shipper.service.time.monotonic",
        lambda: monotonic_now,
    )

    prune_calls: list[tuple[str, int]] = []
    original_prune = shipper._prune_old_rows

    def _recording_prune(*, spec, shipped_through_rowid: int) -> int:
        prune_calls.append((spec.name, shipped_through_rowid))
        return original_prune(spec=spec, shipped_through_rowid=shipped_through_rowid)

    monkeypatch.setattr(shipper, "_prune_old_rows", _recording_prune)

    shipper.ship_once()
    monotonic_now += 1.0
    shipper.ship_once()

    assert [name for name, _rowid in prune_calls] == ["quote_cycle"]


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
    portfolio_row = dict(sink.insert_calls[-1][1][0])
    assert portfolio_row["aggregation_mode"] == "partial"
    assert portfolio_row["stale_required_json"] == '["maker_v3_02"]'
    assert portfolio_row["expected_component_count"] == 2


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
              portfolio_id, base_currency, snapshot_id, snapshot_hash, global_qty_base, global_qty,
              aggregation_mode, global_qty_base_complete, global_qty_complete, degraded,
              missing_required_json, stale_required_json, null_qty_required_json, components_json,
              usable_component_count, expected_component_count, stale_after_ms, ts_ms, ts_ingest_ns, created_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            """,
            (
                "tokenmm",
                "PLUME",
                "portfolio-snapshot-2",
                "portfolio-hash-2",
                "30000",
                "30000",
                "strict",
                1,
                1,
                0,
                "[]",
                "[]",
                "[]",
                '[{"strategy_id":"maker_v3_01","local_qty":"30000"}]',
                2,
                2,
                3000,
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


def test_postgres_sink_ensure_schema_applies_operator_quantity_column_migrations(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    fake_conn = _FakeConnection()
    fake_psycopg = SimpleNamespace(sql=_FakeSQLModule)
    monkeypatch.setitem(sys.modules, "psycopg", fake_psycopg)

    sink = TelemetryPostgresSink(
        TelemetryPostgresConfig(
            host="localhost",
            port=5432,
            database="nautilus_telemetry",
            schema="telemetry",
            username="nautilus",
            password="pass",
            sslmode="require",
        ),
    )
    monkeypatch.setattr(sink, "_get_conn", lambda: fake_conn)

    sink.ensure_schema()

    ddl = "\n".join(fake_conn.executed)
    assert "ALTER TABLE telemetry.execution_fill ADD COLUMN IF NOT EXISTS last_qty_base TEXT" in ddl
    assert "ALTER TABLE telemetry.execution_fill ADD COLUMN IF NOT EXISTS last_qty_venue TEXT" in ddl
    assert "ALTER TABLE telemetry.execution_fill ADD COLUMN IF NOT EXISTS qty_conversion_status TEXT" in ddl
    assert "ALTER TABLE telemetry.execution_fill ADD COLUMN IF NOT EXISTS qty_conversion_source TEXT" in ddl
    assert "ALTER TABLE telemetry.order_action ADD COLUMN IF NOT EXISTS order_qty_base TEXT" in ddl
    assert "ALTER TABLE telemetry.order_action ADD COLUMN IF NOT EXISTS order_qty_venue TEXT" in ddl
    assert "ALTER TABLE telemetry.order_action ADD COLUMN IF NOT EXISTS qty_conversion_status TEXT" in ddl
    assert "ALTER TABLE telemetry.order_action ADD COLUMN IF NOT EXISTS qty_conversion_source TEXT" in ddl
    assert fake_conn.commits == 1


def test_shipper_transports_operator_quantity_fields_for_fills_and_orders(tmp_path: Path) -> None:
    fills_db = tmp_path / "fills.sqlite"
    orders_db = tmp_path / "orders.sqlite"
    state_db = tmp_path / "shipper_state.sqlite"
    _create_execution_fill_source(fills_db)
    _create_order_action_source(orders_db)

    config = TelemetryShipperConfig(
        enabled=True,
        enable_local_persistence=True,
        source_profile="tokenmm",
        balance_snapshots_db_path=None,
        fills_db_path=str(fills_db),
        orders_db_path=str(orders_db),
        quote_cycles_db_path=None,
        portfolio_inventory_db_path=None,
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

    assert result["execution_fill"].shipped == 1
    assert result["order_action"].shipped == 1
    insert_calls = {name: rows for name, rows in sink.insert_calls}
    fill_row = insert_calls["execution_fill"][0]
    order_row = insert_calls["order_action"][0]

    assert fill_row["last_qty"] == "100"
    assert fill_row["last_qty_base"] == "100"
    assert fill_row["last_qty_venue"] == "100"
    assert fill_row["qty_conversion_status"] == "identity"
    assert fill_row["qty_conversion_source"] == "generic:multiplier=1"
    assert fill_row["source_profile"] == "tokenmm"
    assert fill_row["source_host"] == "host-a"

    assert order_row["order_qty"] == "100"
    assert order_row["order_qty_base"] == "100"
    assert order_row["order_qty_venue"] == "100"
    assert order_row["qty_conversion_status"] == "identity"
    assert order_row["qty_conversion_source"] == "generic:multiplier=1"
    assert order_row["source_profile"] == "tokenmm"
    assert order_row["source_host"] == "host-a"


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


def test_shipper_run_exits_78_on_missing_postgres_env(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    config_path = tmp_path / "tokenmm.live.toml"
    config_path.write_text(
        """
[telemetry_shipper]
enabled = true
enable_local_persistence = true
source_profile = "tokenmm"
orders_db_path = "/tmp/orders.sqlite"
state_db_path = "/tmp/shipper_state.sqlite"
""".strip(),
        encoding="utf-8",
    )
    for key in (
        "NAUTILUS_TELEMETRY_PG_HOST",
        "NAUTILUS_TELEMETRY_PG_PORT",
        "NAUTILUS_TELEMETRY_PG_DATABASE",
        "NAUTILUS_TELEMETRY_PG_SCHEMA",
        "NAUTILUS_TELEMETRY_PG_USERNAME",
        "NAUTILUS_TELEMETRY_PG_PASSWORD",
    ):
        monkeypatch.delenv(key, raising=False)
    monkeypatch.setattr(sys, "argv", ["shipper.run", "--config", str(config_path)])

    with pytest.raises(SystemExit) as exc_info:
        shipper_run.main()

    assert exc_info.value.code == 78

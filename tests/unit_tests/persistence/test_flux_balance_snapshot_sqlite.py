from __future__ import annotations

import json


def test_balance_snapshot_normalizer_flattens_accounts_and_positions() -> None:
    from nautilus_trader.flux.persistence.balance_snapshots.normalize import (
        normalize_balance_snapshot,
    )

    record = normalize_balance_snapshot(
        trader_id="TRADER-001",
        topic="flux.makerv3.balances",
        payload={
            "strategy_id": "maker_v3_01",
            "accounts": [
                {
                    "account_id": "BYBIT-001",
                    "events": [
                        {
                            "account_id": "BYBIT-001",
                            "balances": [
                                {"currency": "PLUME", "free": "90", "locked": "10", "total": "100"},
                            ],
                        },
                    ],
                },
            ],
            "positions": [
                {
                    "position_id": "POS-001",
                    "instrument_id": "PLUMEUSDT.BYBIT_SPOT",
                    "signed_qty": "50",
                    "quantity": "50",
                    "side": "LONG",
                    "avg_px_open": "0.12",
                },
            ],
            "ts_event": 123_000_000_000,
            "ts_ms": 123_000,
        },
        ts_ingest_ns=124_000_000_000,
    )

    assert record.snapshot.strategy_id == "maker_v3_01"
    assert record.snapshot.account_count == 1
    assert record.snapshot.position_count == 1
    assert len(record.rows) == 2
    assert {row.kind for row in record.rows} == {"cash", "position"}
    assert any(row.row_key == "bybit:BYBIT-001:PLUME" for row in record.rows)
    assert any(row.row_key == "bybit_spot:PLUMEUSDT.BYBIT_SPOT:POS-001" for row in record.rows)

    payload = json.loads(record.snapshot.payload_json)
    assert payload["strategy_id"] == "maker_v3_01"


def test_balance_snapshot_schema_has_header_and_row_tables(tmp_path) -> None:
    from nautilus_trader.flux.persistence.balance_snapshots.sqlite import connect
    from nautilus_trader.flux.persistence.balance_snapshots.sqlite import ensure_schema

    db_path = tmp_path / "balance_snapshots.sqlite"
    conn = connect(str(db_path))
    ensure_schema(conn)

    tables = {
        row[0]
        for row in conn.execute(
            "SELECT name FROM sqlite_master WHERE type = 'table'",
        ).fetchall()
    }
    assert "flux_balance_snapshot" in tables
    assert "flux_balance_snapshot_row" in tables

    header_columns = {
        row[1]
        for row in conn.execute("PRAGMA table_info(flux_balance_snapshot)").fetchall()
    }
    row_columns = {
        row[1]
        for row in conn.execute("PRAGMA table_info(flux_balance_snapshot_row)").fetchall()
    }

    assert "snapshot_id" in header_columns
    assert "snapshot_hash" in header_columns
    assert "payload_json" in header_columns
    assert "ts_ingest_ns" in header_columns

    assert "snapshot_id" in row_columns
    assert "row_key" in row_columns
    assert "kind" in row_columns
    assert "instrument_id" in row_columns
    assert "account_id" in row_columns
    assert "row_json" in row_columns

    conn.close()

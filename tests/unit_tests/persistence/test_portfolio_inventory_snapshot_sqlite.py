from __future__ import annotations

import sqlite3


def test_portfolio_inventory_snapshot_writer_persists_only_changed_or_heartbeat_rows(tmp_path) -> None:
    from nautilus_trader.flux.persistence.portfolio_inventory_snapshots.sqlite import (
        PortfolioInventorySnapshotWriter,
    )

    db_path = tmp_path / "portfolio_inventory.sqlite"
    writer = PortfolioInventorySnapshotWriter(
        db_path=str(db_path),
        unchanged_heartbeat_ms=5_000,
    )
    try:
        payload = {
            "portfolio_id": "tokenmm",
            "base_currency": "PLUME",
            "global_qty": "10",
            "degraded": False,
            "missing_required": [],
            "components": [],
        }

        assert writer.maybe_persist(payload=payload, ts_ms=1_000) is True
        assert writer.maybe_persist(payload=payload, ts_ms=2_000) is False
        assert writer.maybe_persist(payload=payload, ts_ms=7_000) is True
        assert (
            writer.maybe_persist(
                payload={**payload, "global_qty": "11"},
                ts_ms=8_000,
            )
            is True
        )
    finally:
        writer.close()

    conn = sqlite3.connect(db_path)
    try:
        count = conn.execute("SELECT COUNT(*) FROM portfolio_inventory_snapshot").fetchone()[0]
    finally:
        conn.close()

    assert count == 3


def test_portfolio_inventory_snapshot_writer_tracks_current_inventory_semantics(tmp_path) -> None:
    from nautilus_trader.flux.persistence.portfolio_inventory_snapshots.sqlite import (
        PortfolioInventorySnapshotWriter,
    )

    db_path = tmp_path / "portfolio_inventory.sqlite"
    writer = PortfolioInventorySnapshotWriter(
        db_path=str(db_path),
        unchanged_heartbeat_ms=5_000,
    )
    try:
        payload = {
            "portfolio_id": "tokenmm",
            "base_currency": "PLUME",
            "global_qty_base": "10",
            "global_qty": "10",
            "aggregation_mode": "partial",
            "global_qty_base_complete": False,
            "global_qty_complete": False,
            "stale_after_ms": 3_000,
            "degraded": True,
            "missing_required": [],
            "stale_required": ["plumeusdt_okx_perp_makerv3"],
            "null_qty_required": [],
            "usable_component_count": 1,
            "expected_component_count": 2,
            "components": [
                {
                    "strategy_id": "plumeusdt_bybit_perp_makerv3",
                    "required": True,
                    "fresh": True,
                    "stale": False,
                    "missing": False,
                    "local_qty_base": "10",
                    "local_qty": "10",
                    "ts_ms": 1_000,
                },
            ],
        }

        assert writer.maybe_persist(payload=payload, ts_ms=1_000) is True
        assert (
            writer.maybe_persist(
                payload={
                    **payload,
                    "stale_required": [
                        "plumeusdt_okx_perp_makerv3",
                        "plumeusdt_bitget_perp_makerv3",
                    ],
                    "expected_component_count": 3,
                },
                ts_ms=1_001,
            )
            is True
        )
    finally:
        writer.close()

    conn = sqlite3.connect(db_path)
    try:
        row = conn.execute(
            """
            SELECT
              global_qty_base,
              aggregation_mode,
              global_qty_base_complete,
              global_qty_complete,
              stale_after_ms,
              stale_required_json,
              null_qty_required_json,
              usable_component_count,
              expected_component_count
            FROM portfolio_inventory_snapshot
            ORDER BY ts_ms ASC
            LIMIT 1
            """,
        ).fetchone()
        count = conn.execute("SELECT COUNT(*) FROM portfolio_inventory_snapshot").fetchone()[0]
    finally:
        conn.close()

    assert count == 2
    assert row is not None
    assert row[0] == "10"
    assert row[1] == "partial"
    assert row[2] == 0
    assert row[3] == 0
    assert row[4] == 3_000
    assert row[5] == '["plumeusdt_okx_perp_makerv3"]'
    assert row[6] == "[]"
    assert row[7] == 1
    assert row[8] == 2


def test_portfolio_inventory_snapshot_writer_ignores_component_timestamp_only_churn(
    tmp_path,
) -> None:
    from nautilus_trader.flux.persistence.portfolio_inventory_snapshots.sqlite import (
        PortfolioInventorySnapshotWriter,
    )

    db_path = tmp_path / "portfolio_inventory.sqlite"
    writer = PortfolioInventorySnapshotWriter(
        db_path=str(db_path),
        unchanged_heartbeat_ms=5_000,
    )
    try:
        payload = {
            "portfolio_id": "tokenmm",
            "base_currency": "PLUME",
            "global_qty_base": "30380.87342792",
            "global_qty": "30380.87342792",
            "aggregation_mode": "partial",
            "global_qty_base_complete": True,
            "global_qty_complete": True,
            "stale_after_ms": 3_000,
            "degraded": False,
            "missing_required": [],
            "stale_required": [],
            "null_qty_required": [],
            "usable_component_count": 7,
            "expected_component_count": 7,
            "components": [
                {
                    "strategy_id": "plumeusdt_binance_spot_makerv3",
                    "required": True,
                    "fresh": True,
                    "stale": False,
                    "missing": False,
                    "local_qty_base": "-20733.81960162",
                    "local_qty": "-20733.81960162",
                    "ts_ms": 1_000,
                    "state": "running",
                },
            ],
        }

        assert writer.maybe_persist(payload=payload, ts_ms=1_000) is True
        assert (
            writer.maybe_persist(
                payload={
                    **payload,
                    "components": [
                        {
                            **payload["components"][0],
                            "ts_ms": 1_250,
                        },
                    ],
                },
                ts_ms=1_250,
            )
            is False
        )
    finally:
        writer.close()

    conn = sqlite3.connect(db_path)
    try:
        count = conn.execute("SELECT COUNT(*) FROM portfolio_inventory_snapshot").fetchone()[0]
    finally:
        conn.close()

    assert count == 1

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

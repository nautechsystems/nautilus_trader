from __future__ import annotations


def test_balance_snapshot_sqlite_connect_uses_extended_busy_timeout(tmp_path) -> None:
    from nautilus_trader.flux.persistence.balance_snapshots.sqlite import (
        connect as connect_balance_snapshots,
    )

    conn = connect_balance_snapshots(str(tmp_path / "balance_snapshots.sqlite"))
    try:
        assert conn.execute("PRAGMA busy_timeout;").fetchone()[0] == 30_000
    finally:
        conn.close()


def test_quote_cycles_sqlite_connect_uses_extended_busy_timeout(tmp_path) -> None:
    from nautilus_trader.flux.persistence.quote_cycles.sqlite import (
        connect as connect_quote_cycles,
    )

    conn = connect_quote_cycles(str(tmp_path / "quote_cycles.sqlite"))
    try:
        assert conn.execute("PRAGMA busy_timeout;").fetchone()[0] == 30_000
    finally:
        conn.close()

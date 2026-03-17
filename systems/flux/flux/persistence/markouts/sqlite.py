from __future__ import annotations

import sqlite3
from typing import NamedTuple

from flux.persistence.markouts.schema import EXECUTION_MARKOUT_SCHEMA_SQL
from flux.persistence.markouts.schema import INSERT_EXECUTION_MARKOUT_SQL


class ExecutionMarkoutRow(NamedTuple):
    trader_id: str
    event_id: str
    trade_id: str
    strategy_id: str
    instrument_id: str
    client_order_id: str
    order_side: str
    fill_px: str
    fill_qty: str
    benchmark_name: str
    horizon_s: int
    target_ts_ms: int
    benchmark_ts_ms: int | None
    benchmark_px: str | None
    markout_abs: str | None
    markout_bps: str | None
    resolution_status: str
    run_id: str | None
    quote_cycle_id: str | None
    reason_code: str | None
    level_index: int | None


def connect(path: str) -> sqlite3.Connection:
    conn = sqlite3.connect(path, timeout=5.0)
    conn.execute("PRAGMA journal_mode=WAL;")
    conn.execute("PRAGMA synchronous=NORMAL;")
    return conn


def ensure_schema(conn: sqlite3.Connection) -> None:
    conn.executescript(EXECUTION_MARKOUT_SCHEMA_SQL)


def insert_many(
    conn: sqlite3.Connection,
    rows: list[ExecutionMarkoutRow],
) -> tuple[int, int]:
    if not rows:
        return (0, 0)

    with conn:
        before = conn.total_changes
        conn.executemany(INSERT_EXECUTION_MARKOUT_SQL, rows)
        inserted = conn.total_changes - before

    return inserted, len(rows) - inserted

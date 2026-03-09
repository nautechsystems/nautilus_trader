from __future__ import annotations

import sqlite3

from flux.persistence.balance_snapshots.normalize import FluxBalanceSnapshotRecord
from flux.persistence.balance_snapshots.schema import FLUX_BALANCE_SNAPSHOT_SCHEMA_SQL
from flux.persistence.balance_snapshots.schema import INSERT_FLUX_BALANCE_SNAPSHOT_ROW_SQL
from flux.persistence.balance_snapshots.schema import INSERT_FLUX_BALANCE_SNAPSHOT_SQL


def connect(path: str) -> sqlite3.Connection:
    conn = sqlite3.connect(path, timeout=5.0)
    conn.execute("PRAGMA journal_mode=WAL;")
    conn.execute("PRAGMA synchronous=NORMAL;")
    return conn


def ensure_schema(conn: sqlite3.Connection) -> None:
    conn.executescript(FLUX_BALANCE_SNAPSHOT_SCHEMA_SQL)


def insert_many(
    conn: sqlite3.Connection,
    records: list[FluxBalanceSnapshotRecord],
) -> tuple[int, int]:
    if not records:
        return (0, 0)

    inserted_headers = 0
    with conn:
        for record in records:
            before = conn.total_changes
            conn.execute(
                INSERT_FLUX_BALANCE_SNAPSHOT_SQL,
                (
                    record.snapshot.trader_id,
                    record.snapshot.strategy_id,
                    record.snapshot.snapshot_id,
                    record.snapshot.topic,
                    record.snapshot.snapshot_hash,
                    record.snapshot.ts_event_ns,
                    record.snapshot.ts_ms,
                    record.snapshot.ts_ingest_ns,
                    record.snapshot.account_count,
                    record.snapshot.position_count,
                    record.snapshot.payload_json,
                    record.snapshot.created_at,
                ),
            )
            inserted = conn.total_changes - before
            if inserted <= 0:
                continue
            inserted_headers += 1
            conn.executemany(
                INSERT_FLUX_BALANCE_SNAPSHOT_ROW_SQL,
                [
                    (
                        row.trader_id,
                        row.strategy_id,
                        row.snapshot_id,
                        row.row_key,
                        row.kind,
                        row.exchange,
                        row.account_id,
                        row.account,
                        row.asset,
                        row.instrument_id,
                        row.side,
                        row.signed_qty,
                        row.quantity,
                        row.free,
                        row.locked,
                        row.total,
                        row.avg_px_open,
                        row.avg_px_close,
                        row.realized_pnl,
                        row.ts_ms,
                        row.row_json,
                        row.created_at,
                    )
                    for row in record.rows
                ],
            )

    return inserted_headers, len(records) - inserted_headers

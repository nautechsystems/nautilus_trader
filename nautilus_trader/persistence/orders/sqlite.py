# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------

from __future__ import annotations

import sqlite3
from typing import NamedTuple

from nautilus_trader.persistence.orders.schema import INSERT_ORDER_ACTION_SQL
from nautilus_trader.persistence.orders.schema import ORDER_ACTION_COLUMN_NAMES
from nautilus_trader.persistence.orders.schema import ORDER_ACTION_INDEXES_SQL
from nautilus_trader.persistence.orders.schema import ORDER_ACTION_MIGRATION_DEFAULTS
from nautilus_trader.persistence.orders.schema import ORDER_ACTION_SCHEMA_SQL
from nautilus_trader.persistence.orders.schema import ORDER_ACTION_TABLE_SQL


class OrderActionRow(NamedTuple):
    """
    Primitive SQLite row ordered to match `INSERT_ORDER_ACTION_SQL`.
    """

    trader_id: str
    event_id: str
    strategy_id: str
    instrument_id: str
    client_order_id: str
    account_id: str | None
    venue_order_id: str | None
    position_id: str | None
    action_type: str
    action_state: str
    event_type: str
    action_id: str | None
    action_reason: str | None
    run_id: str | None
    quote_cycle_id: str | None
    reason_code: str | None
    level_index: int | None
    target_px: str | None
    cancel_px: str | None
    match_tol: str | None
    ts_market_data_event_ns: int | None
    ts_market_data_recv_ns: int | None
    ts_decision_ns: int | None
    ts_submit_local_ns: int | None
    ts_command_init_ns: int | None
    ts_risk_recv_ns: int | None
    ts_risk_forward_ns: int | None
    ts_exec_recv_ns: int | None
    ts_exec_forward_ns: int | None
    ts_client_submit_ns: int | None
    ts_adapter_submit_start_ns: int | None
    ts_cancel_request_local_ns: int | None
    decision_context_json: str
    order_side: str | None
    order_type: str | None
    time_in_force: str | None
    post_only: int | None
    reduce_only: int | None
    order_qty: str | None
    order_px: str | None
    rejection_reason: str | None
    ts_event: int
    ts_init: int
    ts_ingest: int
    reconciliation: int
    payload_json: str


def connect(path: str) -> sqlite3.Connection:
    """
    Return a SQLite connection configured for write-heavy append workloads.
    """
    conn = sqlite3.connect(path, timeout=5.0)
    conn.execute("PRAGMA journal_mode=WAL;")
    conn.execute("PRAGMA synchronous=NORMAL;")
    return conn


def ensure_schema(conn: sqlite3.Connection) -> None:
    """
    Ensure the `order_action` schema exists and migrate legacy layouts in place.
    """
    existing = _table_columns(conn, "order_action")
    if not existing:
        conn.executescript(ORDER_ACTION_SCHEMA_SQL)
        return

    desired = set(ORDER_ACTION_COLUMN_NAMES)
    if existing == desired:
        conn.executescript(ORDER_ACTION_INDEXES_SQL)
        return

    _rebuild_order_action_table(conn, existing_columns=existing)


def insert_many(
    conn: sqlite3.Connection,
    rows: list[OrderActionRow],
) -> tuple[int, int]:
    """
    Insert order action rows with idempotency (`ON CONFLICT DO NOTHING`).
    """
    if not rows:
        return (0, 0)

    with conn:
        before = conn.total_changes
        conn.executemany(INSERT_ORDER_ACTION_SQL, rows)
        inserted = conn.total_changes - before

    return inserted, len(rows) - inserted


def _table_columns(conn: sqlite3.Connection, table_name: str) -> set[str]:
    rows = conn.execute(f"PRAGMA table_info({table_name})").fetchall()
    return {row[1] for row in rows}


def _rebuild_order_action_table(
    conn: sqlite3.Connection,
    *,
    existing_columns: set[str],
) -> None:
    migration_selects: list[str] = []
    old_columns = set(existing_columns)

    for column_name in ORDER_ACTION_COLUMN_NAMES:
        if column_name == "decision_context_json":
            if "decision_context_json" in old_columns:
                migration_selects.append(
                    "COALESCE(decision_context_json, 'null') AS decision_context_json",
                )
            elif "signal_snapshot_json" in old_columns:
                migration_selects.append(
                    "COALESCE(signal_snapshot_json, 'null') AS decision_context_json",
                )
            else:
                migration_selects.append("'null' AS decision_context_json")
            continue

        if column_name in old_columns:
            migration_selects.append(column_name)
            continue

        migration_selects.append(
            f"{ORDER_ACTION_MIGRATION_DEFAULTS[column_name]} AS {column_name}",
        )

    target_columns = ", ".join(ORDER_ACTION_COLUMN_NAMES)
    source_select = ", ".join(migration_selects)

    with conn:
        conn.execute("ALTER TABLE order_action RENAME TO order_action__old")
        conn.executescript(ORDER_ACTION_TABLE_SQL)
        conn.execute(
            f"""
            INSERT INTO order_action ({target_columns})
            SELECT {source_select}
            FROM order_action__old
            """,
        )
        conn.execute("DROP TABLE order_action__old")
        conn.executescript(ORDER_ACTION_INDEXES_SQL)

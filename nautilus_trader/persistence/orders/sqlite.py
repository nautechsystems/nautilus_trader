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
from nautilus_trader.persistence.orders.schema import ORDER_ACTION_SCHEMA_SQL


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
    ts_decision_ns: int | None
    signal_snapshot_json: str
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

    Parameters
    ----------
    path : str
        The SQLite DB file path.

    Returns
    -------
    sqlite3.Connection

    """
    conn = sqlite3.connect(path, timeout=5.0)
    conn.execute("PRAGMA journal_mode=WAL;")
    conn.execute("PRAGMA synchronous=NORMAL;")
    return conn


def ensure_schema(conn: sqlite3.Connection) -> None:
    """
    Ensure the `order_action` schema exists.

    Parameters
    ----------
    conn : sqlite3.Connection
        The SQLite connection.

    """
    conn.executescript(ORDER_ACTION_SCHEMA_SQL)


def insert_many(
    conn: sqlite3.Connection,
    rows: list[OrderActionRow],
) -> tuple[int, int]:
    """
    Insert order action rows with idempotency (`ON CONFLICT DO NOTHING`).

    Parameters
    ----------
    conn : sqlite3.Connection
        The SQLite connection.
    rows : list[OrderActionRow]
        Rows to insert in a single transaction.

    Returns
    -------
    tuple[int, int]
        `(inserted_count, deduped_count)`.

    """
    if not rows:
        return (0, 0)

    with conn:
        before = conn.total_changes
        conn.executemany(INSERT_ORDER_ACTION_SQL, rows)
        inserted = conn.total_changes - before

    return inserted, len(rows) - inserted

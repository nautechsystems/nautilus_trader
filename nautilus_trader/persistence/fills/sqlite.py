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
from collections.abc import Callable
from typing import Any

import msgspec

from nautilus_trader.common.config import msgspec_encoding_hook
from nautilus_trader.model.events import OrderFilled
from nautilus_trader.persistence.fills.schema import EXECUTION_FILL_SCHEMA_SQL
from nautilus_trader.persistence.fills.schema import INSERT_EXECUTION_FILL_SQL

ExecutionFillRow = tuple[
    str,
    str,
    str,
    str,
    str,
    str,
    str,
    str,
    str | None,
    str,
    str,
    str,
    str,
    str,
    str,
    str,
    int,
    int,
    int,
    str,
]


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
    Ensure the execution fill schema exists.

    Parameters
    ----------
    conn : sqlite3.Connection
        The SQLite connection.

    """
    conn.executescript(EXECUTION_FILL_SCHEMA_SQL)


def _encode_info_json(
    info: dict[str, Any],
    on_info_encode_error: Callable[[], None] | None = None,
) -> str:
    try:
        return msgspec.json.encode(info, enc_hook=msgspec_encoding_hook).decode("utf-8")
    except Exception:
        if on_info_encode_error is not None:
            on_info_encode_error()
        return "{}"


def fill_to_row(
    fill: OrderFilled,
    on_info_encode_error: Callable[[], None] | None = None,
) -> ExecutionFillRow:
    """
    Convert an `OrderFilled` event to a primitive SQLite row.

    Parameters
    ----------
    fill : OrderFilled
        The fill event.
    on_info_encode_error : Callable[[], None], optional
        Callback to invoke if encoding `fill.info` fails.

    Returns
    -------
    tuple

    """
    data = OrderFilled.to_dict(fill)
    info_json = _encode_info_json(fill.info, on_info_encode_error=on_info_encode_error)

    return (
        data["trader_id"],
        data["event_id"],
        data["strategy_id"],
        data["account_id"],
        data["instrument_id"],
        data["trade_id"],
        data["client_order_id"],
        data["venue_order_id"],
        data["position_id"],
        data["order_side"],
        data["order_type"],
        data["last_qty"],
        data["last_px"],
        data["currency"],
        data["commission"],
        data["liquidity_side"],
        int(data["ts_event"]),
        int(data["ts_init"]),
        int(bool(data["reconciliation"])),
        info_json,
    )


def insert_fills(
    conn: sqlite3.Connection,
    rows: list[ExecutionFillRow],
) -> tuple[int, int]:
    """
    Insert fill rows with idempotency (`ON CONFLICT DO NOTHING`).

    Parameters
    ----------
    conn : sqlite3.Connection
        The SQLite connection.
    rows : list[ExecutionFillRow]
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
        conn.executemany(INSERT_EXECUTION_FILL_SQL, rows)
        inserted = conn.total_changes - before

    return inserted, len(rows) - inserted

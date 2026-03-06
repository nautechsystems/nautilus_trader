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
from nautilus_trader.model.enums import liquidity_side_to_str
from nautilus_trader.model.enums import order_side_to_str
from nautilus_trader.model.enums import order_type_to_str
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
    *,
    info_override: dict[str, Any] | None = None,
    client_order_id_override: str | None = None,
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
    info = fill.info if info_override is None else info_override
    info_json = _encode_info_json(info, on_info_encode_error=on_info_encode_error)
    client_order_id = fill.client_order_id.value
    if client_order_id_override is not None:
        client_order_id = client_order_id_override

    return (
        fill.trader_id.value,
        fill.id.value,
        fill.strategy_id.value,
        fill.account_id.value,
        fill.instrument_id.value,
        fill.trade_id.value,
        client_order_id,
        fill.venue_order_id.value,
        fill.position_id.value if fill.position_id else None,
        order_side_to_str(fill.order_side),
        order_type_to_str(fill.order_type),
        str(fill.last_qty),
        str(fill.last_px),
        fill.currency.code,
        str(fill.commission),
        liquidity_side_to_str(fill.liquidity_side),
        int(fill.ts_event),
        int(fill.ts_init),
        int(bool(fill.reconciliation)),
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

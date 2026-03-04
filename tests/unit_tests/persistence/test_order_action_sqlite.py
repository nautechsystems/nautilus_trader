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

from nautilus_trader.persistence.orders.sqlite import connect
from nautilus_trader.persistence.orders.sqlite import ensure_schema
from nautilus_trader.persistence.orders.sqlite import insert_many


TRADER_ID = "TESTER-001"
STRATEGY_ID = "EMA-001"
INSTRUMENT_ID = "ETHUSDT.BINANCE"


def _row(event_id: str, client_order_id: str, ts_event: int) -> tuple:
    return (
        TRADER_ID,
        event_id,
        STRATEGY_ID,
        INSTRUMENT_ID,
        client_order_id,
        "SIM",
        f"VENUE-{event_id}",
        None,
        "PLACE",
        "SUBMITTED",
        "OrderSubmitted",
        None,
        None,
        None,
        "null",
        "BUY",
        "LIMIT",
        "GTC",
        0,
        0,
        "1.00000000",
        "100.10",
        None,
        ts_event,
        ts_event,
        ts_event + 1,
        0,
        "{}",
    )


def test_insert_many_is_idempotent_on_trader_id_event_id(tmp_path) -> None:
    db_path = tmp_path / "orders.sqlite"
    conn = connect(str(db_path))
    ensure_schema(conn)
    conn.row_factory = sqlite3.Row

    row = _row(event_id="event-001", client_order_id="client-001", ts_event=100)

    first_inserted, first_deduped = insert_many(conn, [row])
    second_inserted, second_deduped = insert_many(conn, [row])

    assert first_inserted == 1
    assert first_deduped == 0
    assert second_inserted == 0
    assert second_deduped == 1

    rows = conn.execute("SELECT trader_id, event_id FROM order_action").fetchall()
    assert len(rows) == 1
    assert rows[0]["trader_id"] == TRADER_ID
    assert rows[0]["event_id"] == "event-001"

    conn.close()


def test_two_events_with_same_client_order_id_are_both_persisted(tmp_path) -> None:
    db_path = tmp_path / "orders.sqlite"
    conn = connect(str(db_path))
    ensure_schema(conn)

    row1 = _row(event_id="event-001", client_order_id="client-001", ts_event=100)
    row2 = _row(event_id="event-002", client_order_id="client-001", ts_event=101)

    inserted, deduped = insert_many(conn, [row1, row2])
    assert inserted == 2
    assert deduped == 0

    total = conn.execute("SELECT COUNT(*) FROM order_action").fetchone()[0]
    same_client_order_id = conn.execute(
        "SELECT COUNT(*) FROM order_action WHERE client_order_id = ?",
        ("client-001",),
    ).fetchone()[0]

    assert total == 2
    assert same_client_order_id == 2

    conn.close()

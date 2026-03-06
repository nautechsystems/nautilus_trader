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

from nautilus_trader.persistence.orders.schema import SIGNAL_SNAPSHOT_JSON_DEFAULT_LITERAL
from nautilus_trader.persistence.orders.sqlite import connect
from nautilus_trader.persistence.orders.sqlite import ensure_schema
from nautilus_trader.persistence.orders.sqlite import insert_many
from nautilus_trader.persistence.orders.sqlite import OrderActionRow


TRADER_ID = "TESTER-001"
STRATEGY_ID = "EMA-001"
INSTRUMENT_ID = "ETHUSDT.BINANCE"


def _row(event_id: str, client_order_id: str, ts_event: int) -> OrderActionRow:
    return OrderActionRow(
        trader_id=TRADER_ID,
        event_id=event_id,
        strategy_id=STRATEGY_ID,
        instrument_id=INSTRUMENT_ID,
        client_order_id=client_order_id,
        account_id="SIM",
        venue_order_id=f"VENUE-{event_id}",
        position_id=None,
        action_type="PLACE",
        action_state="SUBMITTED",
        event_type="OrderSubmitted",
        action_id=None,
        action_reason=None,
        ts_decision_ns=None,
        signal_snapshot_json=SIGNAL_SNAPSHOT_JSON_DEFAULT_LITERAL,
        order_side="BUY",
        order_type="LIMIT",
        time_in_force="GTC",
        post_only=0,
        reduce_only=0,
        order_qty="1.00000000",
        order_px="100.10",
        rejection_reason=None,
        ts_event=ts_event,
        ts_init=ts_event,
        ts_ingest=ts_event + 1,
        reconciliation=0,
        payload_json="{}",
    )


def test_order_action_row_is_constructible_with_named_fields() -> None:
    row = OrderActionRow(
        trader_id=TRADER_ID,
        event_id="event-001",
        strategy_id=STRATEGY_ID,
        instrument_id=INSTRUMENT_ID,
        client_order_id="client-001",
        account_id="SIM",
        venue_order_id="VENUE-event-001",
        position_id=None,
        action_type="PLACE",
        action_state="SUBMITTED",
        event_type="OrderSubmitted",
        action_id=None,
        action_reason=None,
        ts_decision_ns=None,
        signal_snapshot_json=SIGNAL_SNAPSHOT_JSON_DEFAULT_LITERAL,
        order_side="BUY",
        order_type="LIMIT",
        time_in_force="GTC",
        post_only=0,
        reduce_only=0,
        order_qty="1.00000000",
        order_px="100.10",
        rejection_reason=None,
        ts_event=100,
        ts_init=100,
        ts_ingest=101,
        reconciliation=0,
        payload_json="{}",
    )
    assert row.signal_snapshot_json == SIGNAL_SNAPSHOT_JSON_DEFAULT_LITERAL
    assert row.event_id == "event-001"


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


def test_insert_many_with_empty_batch_returns_zero_counts(tmp_path) -> None:
    db_path = tmp_path / "orders.sqlite"
    conn = connect(str(db_path))
    ensure_schema(conn)

    inserted, deduped = insert_many(conn, [])

    assert inserted == 0
    assert deduped == 0
    total = conn.execute("SELECT COUNT(*) FROM order_action").fetchone()[0]
    assert total == 0

    conn.close()


def test_insert_many_with_mixed_duplicate_and_new_rows_counts_correctly(tmp_path) -> None:
    db_path = tmp_path / "orders.sqlite"
    conn = connect(str(db_path))
    ensure_schema(conn)

    existing = _row(event_id="event-001", client_order_id="client-001", ts_event=100)
    duplicate = _row(event_id="event-001", client_order_id="client-001", ts_event=100)
    new = _row(event_id="event-002", client_order_id="client-001", ts_event=101)

    first_inserted, first_deduped = insert_many(conn, [existing])
    inserted, deduped = insert_many(conn, [duplicate, new])

    assert first_inserted == 1
    assert first_deduped == 0
    assert inserted == 1
    assert deduped == 1

    total = conn.execute("SELECT COUNT(*) FROM order_action").fetchone()[0]
    assert total == 2

    conn.close()


def test_schema_default_signal_snapshot_json_is_json_literal_null_not_sql_null(tmp_path) -> None:
    db_path = tmp_path / "orders.sqlite"
    conn = connect(str(db_path))
    ensure_schema(conn)
    conn.row_factory = sqlite3.Row

    conn.execute(
        """
        INSERT INTO order_action (
          trader_id, event_id, strategy_id, instrument_id, client_order_id,
          action_type, action_state, event_type, ts_event, ts_init, ts_ingest
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        """,
        (
            TRADER_ID,
            "event-default-001",
            STRATEGY_ID,
            INSTRUMENT_ID,
            "client-default-001",
            "PLACE",
            "INITIALIZED",
            "OrderInitialized",
            100,
            100,
            101,
        ),
    )

    row = conn.execute(
        "SELECT signal_snapshot_json, signal_snapshot_json IS NULL AS is_null "
        "FROM order_action WHERE event_id = ?",
        ("event-default-001",),
    ).fetchone()
    assert row["signal_snapshot_json"] == SIGNAL_SNAPSHOT_JSON_DEFAULT_LITERAL
    assert row["is_null"] == 0

    conn.close()


def test_schema_creates_index_for_documented_recent_action_queries(tmp_path) -> None:
    db_path = tmp_path / "orders.sqlite"
    conn = connect(str(db_path))
    ensure_schema(conn)

    indexes = {
        row[0]
        for row in conn.execute(
            "SELECT name FROM sqlite_master WHERE type = 'index' AND tbl_name = 'order_action'",
        ).fetchall()
    }

    assert "order_action_trader_strategy_action_state_ts_event_idx" in indexes

    conn.close()

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

from nautilus_trader.persistence.fills.sqlite import connect
from nautilus_trader.persistence.fills.sqlite import ensure_schema
from nautilus_trader.persistence.fills.sqlite import fill_to_row
from nautilus_trader.persistence.fills.sqlite import insert_fills
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.events import TestEventStubs
from nautilus_trader.test_kit.stubs.execution import TestExecStubs


INFO_JSON_INDEX = 19


def _make_fill(instrument, trade_id=None, ts_event: int = 123):
    order = TestExecStubs.make_accepted_order(instrument=instrument)
    return TestEventStubs.order_filled(
        order=order,
        instrument=instrument,
        trade_id=trade_id,
        ts_event=ts_event,
    )


def test_insert_fills_is_idempotent_on_trader_id_event_id(tmp_path) -> None:
    db_path = tmp_path / "fills.sqlite"
    conn = connect(str(db_path))
    ensure_schema(conn)
    conn.row_factory = sqlite3.Row

    instrument = TestInstrumentProvider.btcusdt_binance()
    fill = _make_fill(instrument=instrument)

    row = fill_to_row(fill)
    insert_fills(conn, [row])
    insert_fills(conn, [row])  # Duplicate event_id

    rows = conn.execute("SELECT trader_id, event_id FROM execution_fill").fetchall()
    assert len(rows) == 1
    assert rows[0]["trader_id"] == fill.trader_id.value
    assert rows[0]["event_id"] == fill.id.value

    conn.close()


def test_trade_id_collision_with_distinct_event_ids_persists_both_rows(tmp_path) -> None:
    db_path = tmp_path / "fills.sqlite"
    conn = connect(str(db_path))
    ensure_schema(conn)

    instrument = TestInstrumentProvider.btcusdt_binance()
    fill1 = _make_fill(instrument=instrument, ts_event=100)
    fill2 = _make_fill(instrument=instrument, trade_id=fill1.trade_id, ts_event=101)

    inserted, deduped = insert_fills(conn, [fill_to_row(fill1), fill_to_row(fill2)])
    assert inserted == 2
    assert deduped == 0

    total = conn.execute("SELECT COUNT(*) FROM execution_fill").fetchone()[0]
    same_trade_id = conn.execute(
        "SELECT COUNT(*) FROM execution_fill WHERE trade_id = ?",
        (fill1.trade_id.value,),
    ).fetchone()[0]

    assert total == 2
    assert same_trade_id == 2

    conn.close()


def test_fill_to_row_falls_back_to_empty_info_json_on_encode_error() -> None:
    instrument = TestInstrumentProvider.btcusdt_binance()
    fill = _make_fill(instrument=instrument)
    fill.info["bad"] = object()  # Not msgspec-encodable with the Nautilus hook

    counter = {"count": 0}

    row = fill_to_row(
        fill,
        on_info_encode_error=lambda: counter.__setitem__("count", counter["count"] + 1),
    )

    assert row[INFO_JSON_INDEX] == "{}"
    assert counter["count"] == 1


def test_fill_to_row_maps_core_fields_from_event_attributes() -> None:
    instrument = TestInstrumentProvider.btcusdt_binance()
    fill = _make_fill(instrument=instrument, ts_event=321)

    row = fill_to_row(fill)

    assert row[0] == fill.trader_id.value
    assert row[1] == fill.id.value
    assert row[5] == fill.trade_id.value
    assert row[8] == (fill.position_id.value if fill.position_id else None)
    assert row[16] == 321
    assert row[17] == fill.ts_init

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

from nautilus_trader.model.currencies import USDT
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.instruments import CryptoPerpetual
from nautilus_trader.model.objects import Currency
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.persistence.fills.sqlite import connect
from nautilus_trader.persistence.fills.sqlite import ensure_schema
from nautilus_trader.persistence.fills.sqlite import fill_to_row
from nautilus_trader.persistence.fills.sqlite import insert_fills
from nautilus_trader.persistence._execution_timing import ExecutionTimingRecord
from nautilus_trader.persistence._execution_timing import PLACE_ACTION_TYPE
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.events import TestEventStubs
from nautilus_trader.test_kit.stubs.execution import TestExecStubs


INFO_JSON_INDEX = 19


def _okx_linear_perpetual() -> CryptoPerpetual:
    return CryptoPerpetual(
        instrument_id=InstrumentId(
            symbol=Symbol("PLUME-USDT-SWAP"),
            venue=Venue("OKX"),
        ),
        raw_symbol=Symbol("PLUME-USDT-SWAP"),
        base_currency=Currency.from_str("PLUME"),
        quote_currency=USDT,
        settlement_currency=USDT,
        is_inverse=False,
        price_precision=4,
        size_precision=0,
        price_increment=Price.from_str("0.0001"),
        size_increment=Quantity.from_str("1"),
        multiplier=Quantity.from_str("10"),
        lot_size=Quantity.from_str("1"),
        ts_event=0,
        ts_init=0,
        info={
            "base_exposure_mode": "exact_multiplier",
            "okx_ct_val": "10",
            "okx_ct_val_ccy": "PLUME",
            "okx_ct_type": "linear",
            "okx_lot_sz": "1",
        },
    )


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


def test_fill_to_row_exposes_operator_quantity_fields_for_exact_multiplier_contracts() -> None:
    fill = _make_fill(instrument=_okx_linear_perpetual(), ts_event=654)

    row = fill_to_row(
        fill,
        last_qty_base="1000",
        last_qty_venue="100",
        qty_conversion_status="exact_multiplier",
        qty_conversion_source="generic:multiplier",
    )

    assert row.last_qty == "100"
    assert row.last_qty_venue == "100"
    assert row.last_qty_base == "1000"
    assert row.qty_conversion_status == "exact_multiplier"
    assert row.qty_conversion_source == "generic:multiplier"


def test_fill_to_row_exposes_matching_base_and_venue_qty_fields_for_identity_contracts() -> None:
    fill = _make_fill(instrument=TestInstrumentProvider.btcusdt_binance(), ts_event=655)

    row = fill_to_row(
        fill,
        last_qty_base="100",
        last_qty_venue="100",
        qty_conversion_status="identity",
        qty_conversion_source="generic:multiplier=1",
    )

    assert row.last_qty == "100"
    assert row.last_qty_venue == "100"
    assert row.last_qty_base == "100"
    assert row.qty_conversion_status == "identity"
    assert row.qty_conversion_source == "generic:multiplier=1"


def test_execution_fill_schema_has_ts_ingest_ns_and_intent_correlation_columns(tmp_path) -> None:
    db_path = tmp_path / "fills.sqlite"
    conn = connect(str(db_path))
    ensure_schema(conn)

    columns = {
        row[1]
        for row in conn.execute("PRAGMA table_info(execution_fill)").fetchall()
    }

    assert "run_id" in columns
    assert "quote_cycle_id" in columns
    assert "reason_code" in columns
    assert "level_index" in columns
    assert "target_px" in columns
    assert "cancel_px" in columns
    assert "match_tol" in columns
    assert "ts_market_data_event_ns" in columns
    assert "ts_market_data_recv_ns" in columns
    assert "ts_decision_ns" in columns
    assert "ts_submit_local_ns" in columns
    assert "ts_ingest_ns" in columns
    assert "ts_command_init_ns" in columns
    assert "ts_risk_recv_ns" in columns
    assert "ts_risk_forward_ns" in columns
    assert "ts_exec_recv_ns" in columns
    assert "ts_exec_forward_ns" in columns
    assert "ts_client_submit_ns" in columns
    assert "ts_adapter_submit_start_ns" in columns
    assert "last_qty_base" in columns
    assert "last_qty_venue" in columns
    assert "qty_conversion_status" in columns
    assert "qty_conversion_source" in columns

    conn.close()


def test_execution_fill_schema_migrates_legacy_table_before_creating_new_indexes(tmp_path) -> None:
    db_path = tmp_path / "fills.sqlite"
    conn = sqlite3.connect(db_path)
    conn.executescript(
        """
        CREATE TABLE execution_fill (
          trader_id TEXT NOT NULL,
          event_id TEXT NOT NULL,
          strategy_id TEXT NOT NULL,
          account_id TEXT NOT NULL,
          instrument_id TEXT NOT NULL,
          trade_id TEXT NOT NULL,
          client_order_id TEXT NOT NULL,
          venue_order_id TEXT NOT NULL,
          position_id TEXT,
          order_side TEXT NOT NULL,
          order_type TEXT NOT NULL,
          last_qty TEXT NOT NULL,
          last_px TEXT NOT NULL,
          currency TEXT NOT NULL,
          commission TEXT NOT NULL,
          liquidity_side TEXT NOT NULL,
          ts_event INTEGER NOT NULL,
          ts_init INTEGER NOT NULL,
          reconciliation INTEGER NOT NULL DEFAULT 0,
          info_json TEXT NOT NULL DEFAULT '{}',
          created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
          PRIMARY KEY (trader_id, event_id)
        );
        """,
    )

    ensure_schema(conn)

    columns = {
        row[1]
        for row in conn.execute("PRAGMA table_info(execution_fill)").fetchall()
    }
    indexes = {
        row[1]
        for row in conn.execute("PRAGMA index_list(execution_fill)").fetchall()
    }

    assert "quote_cycle_id" in columns
    assert "ts_submit_gateway_send_ns" in columns
    assert "execution_fill_quote_cycle_id_idx" in indexes

    conn.close()


def test_fill_to_row_maps_generic_execution_timing_columns() -> None:
    instrument = TestInstrumentProvider.btcusdt_binance()
    fill = _make_fill(instrument=instrument, ts_event=555)
    timing = ExecutionTimingRecord(
        strategy_id=fill.strategy_id.value,
        client_order_id=fill.client_order_id.value,
        action_type=PLACE_ACTION_TYPE,
        ts_command_init_ns=100,
        ts_risk_recv_ns=110,
        ts_risk_forward_ns=120,
        ts_exec_recv_ns=130,
        ts_exec_forward_ns=140,
        ts_client_submit_ns=150,
        ts_adapter_submit_start_ns=160,
    )

    row = fill_to_row(fill, execution_timing=timing)

    assert row.ts_command_init_ns == 100
    assert row.ts_risk_recv_ns == 110
    assert row.ts_risk_forward_ns == 120
    assert row.ts_exec_recv_ns == 130
    assert row.ts_exec_forward_ns == 140
    assert row.ts_client_submit_ns == 150
    assert row.ts_adapter_submit_start_ns == 160

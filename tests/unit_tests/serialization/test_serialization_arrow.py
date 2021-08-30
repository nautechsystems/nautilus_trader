# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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

import copy
import os
import sys

import pytest
from fsspec.implementations.memory import MemoryFileSystem

from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.events.risk import TradingStateChanged
from nautilus_trader.common.events.system import ComponentStateChanged
from nautilus_trader.common.factories import OrderFactory
from nautilus_trader.model.c_enums.book_level import BookLevel
from nautilus_trader.model.c_enums.delta_type import DeltaType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.events.account import AccountState
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orderbook.data import OrderBookDelta
from nautilus_trader.model.orderbook.data import OrderBookDeltas
from nautilus_trader.model.orderbook.data import OrderBookSnapshot
from nautilus_trader.model.position import Position
from nautilus_trader.persistence.catalog import DataCatalog
from nautilus_trader.persistence.external.core import write_chunk
from nautilus_trader.serialization.arrow.serializer import ParquetSerializer
from nautilus_trader.serialization.arrow.util import class_to_filename
from tests.test_kit.providers import TestInstrumentProvider
from tests.test_kit.stubs import TestStubs


AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")
ETHUSDT_BINANCE = TestInstrumentProvider.ethusdt_binance()


def _reset():
    """Cleanup resources before each test run"""
    os.environ["NAUTILUS_CATALOG"] = "memory:///root/"
    catalog = DataCatalog.from_env()
    assert isinstance(catalog.fs, MemoryFileSystem)
    try:
        catalog.fs.rm("/", recursive=True)
    except FileNotFoundError:
        pass
    catalog.fs.mkdir("/root/data")
    assert catalog.fs.exists("/root/")


@pytest.mark.skipif(sys.platform == "win32", reason="does not run on windows")
class TestParquetSerializer:
    def setup(self):
        # Fixture Setup
        _reset()
        self.catalog = DataCatalog(path="/root", fs_protocol="memory")
        self.order_factory = OrderFactory(
            trader_id=TraderId("T-001"),
            strategy_id=StrategyId("S-001"),
            clock=TestClock(),
        )
        self.order = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )
        self.order_submitted = copy.copy(self.order)
        self.order_submitted.apply(TestStubs.event_order_submitted(self.order))

        self.order_accepted = copy.copy(self.order_submitted)
        self.order_accepted.apply(TestStubs.event_order_accepted(self.order_submitted))

        self.order_pending_cancel = copy.copy(self.order_accepted)
        self.order_pending_cancel.apply(TestStubs.event_order_pending_cancel(self.order_accepted))

        self.order_cancelled = copy.copy(self.order_pending_cancel)
        self.order_cancelled.apply(TestStubs.event_order_canceled(self.order_pending_cancel))

    @pytest.mark.parametrize(
        "tick",
        [
            TestStubs.ticker(),
            TestStubs.quote_tick_5decimal(),
            TestStubs.trade_tick_5decimal(),
        ],
    )
    def test_serialize_and_deserialize_tick(self, tick):
        serialized = ParquetSerializer.serialize(tick)
        deserialized = ParquetSerializer.deserialize(cls=type(tick), chunk=[serialized])

        # Assert
        assert deserialized == [tick]
        write_chunk(catalog=self.catalog, chunk=[tick])

    def test_serialize_and_deserialize_order_book_delta(self):
        delta = OrderBookDelta(
            instrument_id=TestStubs.audusd_id(),
            level=BookLevel.L2,
            delta_type=DeltaType.CLEAR,
            order=None,
            ts_event=0,
            ts_init=0,
        )

        serialized = ParquetSerializer.serialize(delta)
        [deserialized] = ParquetSerializer.deserialize(cls=OrderBookDelta, chunk=serialized)

        # Assert
        expected = OrderBookDeltas(
            instrument_id=TestStubs.audusd_id(),
            level=BookLevel.L2,
            deltas=[delta],
            ts_event=0,
            ts_init=0,
        )
        assert deserialized == expected
        write_chunk(catalog=self.catalog, chunk=[delta])

    def test_serialize_and_deserialize_order_book_deltas(self):
        kw = {
            "instrument_id": "AUD/USD.SIM",
            "ts_event": 0,
            "ts_init": 0,
            "level": "L2",
        }
        deltas = OrderBookDeltas(
            instrument_id=TestStubs.audusd_id(),
            level=BookLevel.L2,
            deltas=[
                OrderBookDelta.from_dict(
                    {
                        "delta_type": "ADD",
                        "order_side": "BUY",
                        "order_price": 8.0,
                        "order_size": 30.0,
                        "order_id": "e0364f94-8fcb-0262-cbb3-075c51ee4917",
                        **kw,
                    }
                ),
                OrderBookDelta.from_dict(
                    {
                        "delta_type": "ADD",
                        "order_side": "SELL",
                        "order_price": 15.0,
                        "order_size": 10.0,
                        "order_id": "cabec174-acc6-9204-9ebf-809da3896daf",
                        **kw,
                    }
                ),
            ],
            ts_event=0,
            ts_init=0,
        )

        serialized = ParquetSerializer.serialize(deltas)
        deserialized = ParquetSerializer.deserialize(cls=OrderBookDeltas, chunk=serialized)

        # Assert
        assert deserialized == [deltas]
        write_chunk(catalog=self.catalog, chunk=[deltas])

    def test_serialize_and_deserialize_order_book_deltas_grouped(self):
        kw = {
            "instrument_id": "AUD/USD.SIM",
            "ts_event": 0,
            "ts_init": 0,
            "level": "L2",
        }
        deltas = [
            {
                "delta_type": "ADD",
                "order_side": "SELL",
                "order_price": 0.9901,
                "order_size": 327.25,
                "order_id": "1",
            },
            {
                "delta_type": "CLEAR",
                "order_side": None,
                "order_price": None,
                "order_size": None,
                "order_id": None,
            },
            {
                "delta_type": "ADD",
                "order_side": "SELL",
                "order_price": 0.98039,
                "order_size": 27.91,
                "order_id": "2",
            },
            {
                "delta_type": "ADD",
                "order_side": "SELL",
                "order_price": 0.97087,
                "order_size": 14.43,
                "order_id": "3",
            },
        ]
        deltas = OrderBookDeltas(
            instrument_id=TestStubs.audusd_id(),
            level=BookLevel.L2,
            deltas=[OrderBookDelta.from_dict({**kw, **d}) for d in deltas],
            ts_event=0,
            ts_init=0,
        )

        serialized = ParquetSerializer.serialize(deltas)
        [deserialized] = ParquetSerializer.deserialize(cls=OrderBookDeltas, chunk=serialized)

        # Assert
        assert deserialized == deltas
        write_chunk(catalog=self.catalog, chunk=[deserialized])
        assert [d.type for d in deserialized.deltas] == [
            DeltaType.ADD,
            DeltaType.CLEAR,
            DeltaType.ADD,
            DeltaType.ADD,
        ]

    def test_serialize_and_deserialize_order_book_snapshot(self):
        book = TestStubs.order_book_snapshot()

        serialized = ParquetSerializer.serialize(book)
        deserialized = ParquetSerializer.deserialize(cls=OrderBookSnapshot, chunk=serialized)

        # Assert
        assert deserialized == [book]
        write_chunk(catalog=self.catalog, chunk=[book])

    def test_serialize_and_deserialize_component_state_changed(self):
        event = TestStubs.event_component_state_changed()

        serialized = ParquetSerializer.serialize(event)
        [deserialized] = ParquetSerializer.deserialize(
            cls=ComponentStateChanged, chunk=[serialized]
        )

        # Assert
        assert deserialized == event

        write_chunk(catalog=self.catalog, chunk=[event])

    def test_serialize_and_deserialize_trading_state_changed(self):
        event = TestStubs.event_trading_state_changed()

        serialized = ParquetSerializer.serialize(event)
        [deserialized] = ParquetSerializer.deserialize(cls=TradingStateChanged, chunk=[serialized])

        # Assert
        assert deserialized == event

        write_chunk(catalog=self.catalog, chunk=[event])

    def test_serialize_and_deserialize_account_state(self):
        event = TestStubs.event_cash_account_state()

        serialized = ParquetSerializer.serialize(event)
        [deserialized] = ParquetSerializer.deserialize(cls=AccountState, chunk=serialized)

        # Assert
        assert deserialized == event

        write_chunk(catalog=self.catalog, chunk=[event])

    @pytest.mark.parametrize(
        "event_func",
        [
            TestStubs.event_order_accepted,
            TestStubs.event_order_rejected,
            TestStubs.event_order_submitted,
        ],
    )
    def test_serialize_and_deserialize_order_events_base(self, event_func):
        order = TestStubs.limit_order()
        # order.venue_order_id = "1"
        event = event_func(order=order)
        cls = type(event)

        serialized = ParquetSerializer.serialize(event)
        deserialized = ParquetSerializer.deserialize(cls=cls, chunk=[serialized])

        # Assert
        assert deserialized == [event]
        write_chunk(catalog=self.catalog, chunk=[event])
        df = self.catalog._query(path=f"data/{class_to_filename(cls)}.parquet")
        assert len(df) == 1

    @pytest.mark.parametrize(
        "event_func",
        [
            TestStubs.event_order_canceled,
            TestStubs.event_order_expired,
            TestStubs.event_order_pending_cancel,
            TestStubs.event_order_pending_update,
            TestStubs.event_order_triggered,
        ],
    )
    def test_serialize_and_deserialize_order_events_post_accepted(self, event_func):
        # Act
        event = event_func(order=self.order_accepted)
        cls = type(event)

        serialized = ParquetSerializer.serialize(event)
        deserialized = ParquetSerializer.deserialize(cls=cls, chunk=[serialized])

        # Assert
        assert deserialized == [event]
        write_chunk(catalog=self.catalog, chunk=[event])
        df = self.catalog._query(path=f"data/{class_to_filename(cls)}.parquet")
        assert len(df) == 1

    @pytest.mark.parametrize(
        "event_func",
        [
            TestStubs.event_order_filled,
        ],
    )
    def test_serialize_and_deserialize_order_events_filled(self, event_func):
        # Act
        event = event_func(order=self.order_accepted, instrument=AUDUSD_SIM)
        cls = type(event)

        serialized = ParquetSerializer.serialize(event)
        assert serialized
        # TODO (bm) - can't deserialize order filled right now
        # deserialized = ParquetSerializer.deserialize(cls=cls, chunk=serialized)

        # Assert
        # assert deserialized == [event]
        write_chunk(catalog=self.catalog, chunk=[event])
        df = self.catalog._query(path=f"data/{class_to_filename(cls)}.parquet")
        assert len(df) == 1

    @pytest.mark.parametrize(
        "position_func",
        [
            TestStubs.event_position_opened,
            TestStubs.event_position_changed,
        ],
    )
    def test_serialize_and_deserialize_position_events_open_changed(self, position_func):
        instrument = TestInstrumentProvider.default_fx_ccy("GBPUSD")

        order3 = self.order_factory.market(
            instrument.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )
        fill3 = TestStubs.event_order_filled(
            order3,
            instrument=instrument,
            position_id=PositionId("P-3"),
            strategy_id=StrategyId("S-1"),
            last_px=Price.from_str("1.00000"),
        )

        position = Position(instrument=instrument, fill=fill3)

        event = position_func(position=position)
        cls = type(event)

        serialized = ParquetSerializer.serialize(event)
        assert serialized
        # TODO (bm) - can't deserialize positions right now
        # deserialized = ParquetSerializer.deserialize(cls=cls, chunk=serialized)

        # Assert
        # assert deserialized == [event]
        write_chunk(catalog=self.catalog, chunk=[event])
        df = self.catalog._query(path=f"data/{class_to_filename(cls)}.parquet")
        assert len(df) == 1

    @pytest.mark.parametrize(
        "position_func",
        [
            TestStubs.event_position_closed,
        ],
    )
    def test_serialize_and_deserialize_position_events_closed(self, position_func):
        instrument = TestInstrumentProvider.default_fx_ccy("GBPUSD")

        open_order = self.order_factory.market(
            instrument.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )
        open_fill = TestStubs.event_order_filled(
            open_order,
            instrument=instrument,
            position_id=PositionId("P-3"),
            strategy_id=StrategyId("S-1"),
            last_px=Price.from_str("1.00000"),
        )
        close_order = self.order_factory.market(
            instrument.id,
            OrderSide.SELL,
            Quantity.from_int(100000),
        )
        close_fill = TestStubs.event_order_filled(
            close_order,
            instrument=instrument,
            position_id=PositionId("P-3"),
            strategy_id=StrategyId("S-1"),
            last_px=Price.from_str("1.20000"),
        )

        position = Position(instrument=instrument, fill=open_fill)
        position.apply(close_fill)

        event = position_func(position=position)
        cls = type(event)

        serialized = ParquetSerializer.serialize(event)
        assert serialized
        # TODO (bm) - can't deserialize positions right now
        # deserialized = ParquetSerializer.deserialize(cls=cls, chunk=serialized)

        # Assert
        # assert deserialized == [event]
        write_chunk(catalog=self.catalog, chunk=[event])
        df = self.catalog._query(path=f"data/{class_to_filename(cls)}.parquet")
        assert len(df) == 1

    @pytest.mark.parametrize(
        "instrument",
        [
            TestInstrumentProvider.btcusdt_binance(),
            TestInstrumentProvider.aapl_equity(),
            TestInstrumentProvider.es_future(),
            TestInstrumentProvider.aapl_option(),
        ],
    )
    def test_serialize_and_deserialize_instruments(self, instrument):
        serialized = ParquetSerializer.serialize(instrument)
        assert serialized
        deserialized = ParquetSerializer.deserialize(cls=type(instrument), chunk=[serialized])

        # Assert
        assert deserialized == [instrument]
        write_chunk(catalog=self.catalog, chunk=[instrument])
        df = self.catalog.instruments()
        assert len(df) == 1

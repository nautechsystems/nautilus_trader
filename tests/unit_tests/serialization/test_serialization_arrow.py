# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

import contextlib
import copy
import os
from typing import Any

import pytest
from fsspec.implementations.memory import MemoryFileSystem

from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.factories import OrderFactory
from nautilus_trader.common.messages import ComponentStateChanged
from nautilus_trader.common.messages import TradingStateChanged
from nautilus_trader.model.data.book import OrderBookDelta
from nautilus_trader.model.data.book import OrderBookDeltas
from nautilus_trader.model.data.book import OrderBookSnapshot
from nautilus_trader.model.enums import BookAction
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.events.account import AccountState
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.position import Position
from nautilus_trader.persistence.catalog.parquet import ParquetDataCatalog
from nautilus_trader.persistence.external.core import write_objects
from nautilus_trader.serialization.arrow.serializer import ParquetSerializer
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.data import TestDataStubs
from nautilus_trader.test_kit.stubs.events import TestEventStubs
from nautilus_trader.test_kit.stubs.execution import TestExecStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs
from tests.unit_tests.serialization.conftest import nautilus_objects


AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")
ETHUSDT_BINANCE = TestInstrumentProvider.ethusdt_binance()


def _reset():
    """Cleanup resources before each test run"""
    os.environ["NAUTILUS_PATH"] = "memory:///.nautilus/"
    catalog = ParquetDataCatalog.from_env()
    assert isinstance(catalog.fs, MemoryFileSystem)
    with contextlib.suppress(FileNotFoundError):
        catalog.fs.rm("/", recursive=True)

    catalog.fs.mkdir("/.nautilus/catalog")
    assert catalog.fs.exists("/.nautilus/catalog/")


class TestParquetSerializer:
    def setup(self):
        # Fixture Setup
        _reset()
        self.catalog = ParquetDataCatalog(path="/root", fs_protocol="memory")
        self.order_factory = OrderFactory(
            trader_id=TraderId("T-001"),
            strategy_id=StrategyId("S-001"),
            clock=TestClock(),
        )
        self.order = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )
        self.order_submitted = copy.copy(self.order)
        self.order_submitted.apply(TestEventStubs.order_submitted(self.order))

        self.order_accepted = copy.copy(self.order_submitted)
        self.order_accepted.apply(TestEventStubs.order_accepted(self.order_submitted))

        self.order_updated = copy.copy(self.order_submitted)
        self.order_updated.apply(
            TestEventStubs.order_updated(
                self.order,
                price=Price.from_str("1.00000"),
                quantity=Quantity.from_int(1),
            ),
        )

        self.order_pending_cancel = copy.copy(self.order_accepted)
        self.order_pending_cancel.apply(TestEventStubs.order_pending_cancel(self.order_accepted))

        self.order_cancelled = copy.copy(self.order_pending_cancel)
        self.order_cancelled.apply(TestEventStubs.order_canceled(self.order_pending_cancel))

    def _test_serialization(self, obj: Any):
        cls = type(obj)
        serialized = ParquetSerializer.serialize(obj)
        if not isinstance(serialized, list):
            serialized = [serialized]
        deserialized = ParquetSerializer.deserialize(cls=cls, chunk=serialized)

        # Assert
        expected = obj
        if isinstance(deserialized, list) and not isinstance(expected, list):
            expected = [expected]
        assert deserialized == expected
        write_objects(catalog=self.catalog, chunk=[obj])
        df = self.catalog._query(cls=cls)
        assert len(df) in (1, 2)
        nautilus = self.catalog._query(cls=cls, as_dataframe=False)[0]
        assert nautilus.ts_init == 0
        return True

    @pytest.mark.parametrize(
        "tick",
        [
            TestDataStubs.ticker(),
            TestDataStubs.quote_tick_5decimal(),
            TestDataStubs.trade_tick_5decimal(),
        ],
    )
    def test_serialize_and_deserialize_tick(self, tick):
        self._test_serialization(obj=tick)

    def test_serialize_and_deserialize_bar(self):
        bar = TestDataStubs.bar_5decimal()
        self._test_serialization(obj=bar)

    def test_serialize_and_deserialize_order_book_delta(self):
        delta = OrderBookDelta(
            instrument_id=TestIdStubs.audusd_id(),
            action=BookAction.CLEAR,
            order=None,
            ts_event=0,
            ts_init=0,
        )

        serialized = ParquetSerializer.serialize(delta)
        [deserialized] = ParquetSerializer.deserialize(cls=OrderBookDelta, chunk=serialized)

        # Assert
        expected = OrderBookDeltas(
            instrument_id=TestIdStubs.audusd_id(),
            deltas=[delta],
            ts_event=0,
            ts_init=0,
        )
        assert deserialized == expected
        write_objects(catalog=self.catalog, chunk=[delta])

    def test_serialize_and_deserialize_order_book_deltas(self):
        kw = {
            "instrument_id": "AUD/USD.SIM",
            "ts_event": 0,
            "ts_init": 0,
        }
        deltas = OrderBookDeltas(
            instrument_id=TestIdStubs.audusd_id(),
            deltas=[
                OrderBookDelta.from_dict(
                    {
                        "action": "ADD",
                        "side": "BUY",
                        "price": 8.0,
                        "size": 30.0,
                        "order_id": "e0364f94-8fcb-0262-cbb3-075c51ee4917",
                        **kw,
                    },
                ),
                OrderBookDelta.from_dict(
                    {
                        "action": "ADD",
                        "side": "SELL",
                        "price": 15.0,
                        "size": 10.0,
                        "order_id": "cabec174-acc6-9204-9ebf-809da3896daf",
                        **kw,
                    },
                ),
            ],
            ts_event=0,
            ts_init=0,
        )

        serialized = ParquetSerializer.serialize(deltas)
        deserialized = ParquetSerializer.deserialize(cls=OrderBookDeltas, chunk=serialized)

        # Assert
        assert deserialized == [deltas]
        write_objects(catalog=self.catalog, chunk=[deltas])

    def test_serialize_and_deserialize_order_book_deltas_grouped(self):
        kw = {
            "instrument_id": "AUD/USD.SIM",
            "ts_event": 0,
            "ts_init": 0,
        }
        deltas = [
            {
                "action": "ADD",
                "side": "SELL",
                "price": 0.9901,
                "size": 327.25,
                "order_id": "1",
            },
            {
                "action": "CLEAR",
                "side": None,
                "price": None,
                "size": None,
                "order_id": None,
            },
            {
                "action": "ADD",
                "side": "SELL",
                "price": 0.98039,
                "size": 27.91,
                "order_id": "2",
            },
            {
                "action": "ADD",
                "side": "SELL",
                "price": 0.97087,
                "size": 14.43,
                "order_id": "3",
            },
        ]
        deltas = OrderBookDeltas(
            instrument_id=TestIdStubs.audusd_id(),
            deltas=[OrderBookDelta.from_dict({**kw, **d}) for d in deltas],
            ts_event=0,
            ts_init=0,
        )

        serialized = ParquetSerializer.serialize(deltas)
        [deserialized] = ParquetSerializer.deserialize(cls=OrderBookDeltas, chunk=serialized)

        # Assert
        assert deserialized == deltas
        write_objects(catalog=self.catalog, chunk=[deserialized])
        assert [d.action for d in deserialized.deltas] == [
            BookAction.ADD,
            BookAction.CLEAR,
            BookAction.ADD,
            BookAction.ADD,
        ]

    def test_serialize_and_deserialize_order_book_snapshot(self):
        book = TestDataStubs.order_book_snapshot()

        serialized = ParquetSerializer.serialize(book)
        deserialized = ParquetSerializer.deserialize(cls=OrderBookSnapshot, chunk=serialized)

        # Assert
        assert deserialized == [book]
        write_objects(catalog=self.catalog, chunk=[book])

    def test_serialize_and_deserialize_component_state_changed(self):
        event = TestEventStubs.component_state_changed()

        serialized = ParquetSerializer.serialize(event)
        [deserialized] = ParquetSerializer.deserialize(
            cls=ComponentStateChanged,
            chunk=[serialized],
        )

        # Assert
        assert deserialized == event

        write_objects(catalog=self.catalog, chunk=[event])

    def test_serialize_and_deserialize_trading_state_changed(self):
        event = TestEventStubs.trading_state_changed()

        serialized = ParquetSerializer.serialize(event)
        [deserialized] = ParquetSerializer.deserialize(cls=TradingStateChanged, chunk=[serialized])

        # Assert
        assert deserialized == event

        write_objects(catalog=self.catalog, chunk=[event])

    @pytest.mark.parametrize(
        "event",
        [
            TestEventStubs.cash_account_state(),
            TestEventStubs.margin_account_state(),
        ],
    )
    def test_serialize_and_deserialize_account_state(self, event):
        serialized = ParquetSerializer.serialize(event)
        [deserialized] = ParquetSerializer.deserialize(cls=AccountState, chunk=serialized)

        # Assert
        assert deserialized == event

        write_objects(catalog=self.catalog, chunk=[event])

    @pytest.mark.parametrize(
        "event_func",
        [
            TestEventStubs.order_accepted,
            TestEventStubs.order_rejected,
            TestEventStubs.order_submitted,
        ],
    )
    def test_serialize_and_deserialize_order_events_base(self, event_func):
        order = TestExecStubs.limit_order()
        event = event_func(order=order)
        self._test_serialization(obj=event)

    def test_serialize_and_deserialize_order_updated_events(self):
        order = TestExecStubs.limit_order()
        event = TestEventStubs.order_updated(
            order=order,
            quantity=Quantity.from_int(500_000),
            price=Price.from_str("1.00000"),
        )
        self._test_serialization(obj=event)

    @pytest.mark.parametrize(
        "event_func",
        [
            TestEventStubs.order_submitted,
            TestEventStubs.order_accepted,
            TestEventStubs.order_canceled,
            TestEventStubs.order_pending_update,
            TestEventStubs.order_pending_cancel,
            TestEventStubs.order_triggered,
            TestEventStubs.order_expired,
            TestEventStubs.order_rejected,
            TestEventStubs.order_canceled,
        ],
    )
    def test_serialize_and_deserialize_order_events_post_accepted(self, event_func):
        # Act
        event = event_func(order=self.order_accepted)
        assert self._test_serialization(obj=event)

    @pytest.mark.parametrize(
        "event_func",
        [
            TestEventStubs.order_filled,
        ],
    )
    def test_serialize_and_deserialize_order_events_filled(self, event_func):
        # Act
        event = event_func(order=self.order_accepted, instrument=AUDUSD_SIM)
        self._test_serialization(obj=event)

    @pytest.mark.parametrize(
        "position_func",
        [
            TestEventStubs.position_opened,
            TestEventStubs.position_changed,
        ],
    )
    def test_serialize_and_deserialize_position_events_open_changed(self, position_func):
        instrument = TestInstrumentProvider.default_fx_ccy("GBPUSD")

        order3 = self.order_factory.market(
            instrument.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )
        fill3 = TestEventStubs.order_filled(
            order3,
            instrument=instrument,
            position_id=PositionId("P-3"),
            strategy_id=StrategyId("S-1"),
            last_px=Price.from_str("1.00000"),
        )

        position = Position(instrument=instrument, fill=fill3)

        event = position_func(position=position)
        self._test_serialization(obj=event)

    @pytest.mark.parametrize(
        "position_func",
        [
            TestEventStubs.position_closed,
        ],
    )
    def test_serialize_and_deserialize_position_events_closed(self, position_func):
        instrument = TestInstrumentProvider.default_fx_ccy("GBPUSD")

        open_order = self.order_factory.market(
            instrument.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )
        open_fill = TestEventStubs.order_filled(
            open_order,
            instrument=instrument,
            position_id=PositionId("P-3"),
            strategy_id=StrategyId("S-1"),
            last_px=Price.from_str("1.00000"),
        )
        close_order = self.order_factory.market(
            instrument.id,
            OrderSide.SELL,
            Quantity.from_int(100_000),
        )
        close_fill = TestEventStubs.order_filled(
            close_order,
            instrument=instrument,
            position_id=PositionId("P-3"),
            strategy_id=StrategyId("S-1"),
            last_px=Price.from_str("1.20000"),
        )

        position = Position(instrument=instrument, fill=open_fill)
        position.apply(close_fill)

        event = position_func(position=position)
        self._test_serialization(obj=event)

    @pytest.mark.parametrize(
        "instrument",
        [
            TestInstrumentProvider.xbtusd_bitmex(),
            TestInstrumentProvider.btcusdt_future_binance(),
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
        write_objects(catalog=self.catalog, chunk=[instrument])
        df = self.catalog.instruments()
        assert len(df) == 1

    @pytest.mark.parametrize("obj", nautilus_objects())
    def test_serialize_and_deserialize_all(self, obj):
        # Arrange, Act
        assert self._test_serialization(obj)

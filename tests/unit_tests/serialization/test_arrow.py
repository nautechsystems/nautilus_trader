# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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
import sys
from typing import Any

import pytest

from nautilus_trader.common.component import TestClock
from nautilus_trader.common.factories import OrderFactory
from nautilus_trader.common.messages import ComponentStateChanged
from nautilus_trader.common.messages import TradingStateChanged
from nautilus_trader.model.data import OrderBookDelta
from nautilus_trader.model.data import OrderBookDeltas
from nautilus_trader.model.enums import BookAction
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.events import AccountState
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.position import Position
from nautilus_trader.persistence.catalog.parquet import ParquetDataCatalog
from nautilus_trader.serialization.arrow.serializer import ArrowSerializer
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.data import TestDataStubs
from nautilus_trader.test_kit.stubs.events import TestEventStubs
from nautilus_trader.test_kit.stubs.execution import TestExecStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs
from tests import TESTS_PACKAGE_ROOT
from tests.unit_tests.serialization.conftest import nautilus_objects


AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")
ETHUSDT_BINANCE = TestInstrumentProvider.ethusdt_binance()
CATALOG_PATH = TESTS_PACKAGE_ROOT / "unit_tests" / "persistence" / "catalog"


def _reset(catalog: ParquetDataCatalog) -> None:
    """
    Cleanup resources before each test run.
    """
    assert catalog.path.endswith("tests/unit_tests/persistence/catalog")
    if catalog.fs.exists(catalog.path):
        catalog.fs.rm(catalog.path, recursive=True)
    catalog.fs.mkdir(catalog.path)
    assert catalog.fs.exists(catalog.path)


@pytest.mark.skipif(sys.platform == "win32", reason="Failing on windows")
class TestArrowSerializer:
    def setup(self):
        # Fixture Setup
        self.catalog = ParquetDataCatalog(path=str(CATALOG_PATH), fs_protocol="file")
        _reset(self.catalog)
        self.order_factory = OrderFactory(
            trader_id=TraderId("T-001"),
            strategy_id=StrategyId("S-001"),
            clock=TestClock(),
        )
        self.order = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            tags=["tag-01", "tag-02", "tag-03"],
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

    def _test_serialization(self, obj: Any) -> bool:
        data_cls = type(obj)
        serialized = ArrowSerializer.serialize(obj)
        deserialized = ArrowSerializer.deserialize(data_cls, serialized)

        # Assert
        expected = obj
        if isinstance(deserialized, list) and not isinstance(expected, list):
            expected = [expected]
        # TODO - Can't compare rust vs python types?
        # assert deserialized == expected
        self.catalog.write_data([obj])
        df = self.catalog.query(data_cls=data_cls)
        assert len(df) in (1, 2)
        nautilus = self.catalog.query(data_cls=data_cls, as_dataframe=False)[0]
        assert nautilus.ts_init == 0
        return True

    @pytest.mark.parametrize(
        "data",
        [
            TestDataStubs.quote_tick(),
            TestDataStubs.trade_tick(),
            TestDataStubs.bar_5decimal(),
        ],
    )
    def test_serialize_and_deserialize_tick(self, data):
        self._test_serialization(obj=data)

    def test_serialize_and_deserialize_order_book_delta(self):
        # Arrange
        delta = OrderBookDelta(
            instrument_id=TestIdStubs.audusd_id(),
            action=BookAction.CLEAR,
            order=None,
            flags=0,
            sequence=0,
            ts_event=0,
            ts_init=0,
        )

        # Act
        serialized = ArrowSerializer.serialize(delta)
        deserialized = ArrowSerializer.deserialize(data_cls=OrderBookDelta, batch=serialized)

        # Assert
        OrderBookDeltas(
            instrument_id=TestIdStubs.audusd_id(),
            deltas=[delta],
        )
        self.catalog.write_data([delta])
        deltas = self.catalog.order_book_deltas()
        assert len(deltas) == 1
        assert isinstance(deltas[0], OrderBookDelta)
        assert not isinstance(deserialized[0], OrderBookDelta)  # TODO: Legacy wrangler

    def test_serialize_and_deserialize_order_book_deltas(self):
        # Arrange
        deltas = OrderBookDeltas(
            instrument_id=TestIdStubs.audusd_id(),
            deltas=[
                OrderBookDelta.from_dict(
                    {
                        "instrument_id": "AUD/USD.SIM",
                        "action": "ADD",
                        "order": {
                            "side": "BUY",
                            "price": "8.0",
                            "size": "30.0",
                            "order_id": 1,
                        },
                        "flags": 0,
                        "sequence": 0,
                        "ts_event": 0,
                        "ts_init": 0,
                    },
                ),
                OrderBookDelta.from_dict(
                    {
                        "instrument_id": "AUD/USD.SIM",
                        "action": "ADD",
                        "order": {
                            "side": "SELL",
                            "price": "15.0",
                            "size": "10.0",
                            "order_id": 1,
                        },
                        "flags": 0,
                        "sequence": 0,
                        "ts_event": 0,
                        "ts_init": 0,
                    },
                ),
            ],
        )

        # Act
        serialized = ArrowSerializer.serialize(deltas)
        deserialized = ArrowSerializer.deserialize(data_cls=OrderBookDeltas, batch=serialized)

        self.catalog.write_data(deserialized)

        # Assert
        assert len(deserialized) == 2
        # assert len(self.catalog.order_book_deltas()) == 1

    def test_serialize_and_deserialize_order_book_deltas_grouped(self):
        # Arrange
        kw = {
            "instrument_id": "AUD/USD.SIM",
            "ts_event": 0,
            "ts_init": 0,
        }
        deltas = [
            {
                "action": "ADD",
                "order": {
                    "side": "SELL",
                    "price": "0.9901",
                    "size": "327.25",
                    "order_id": 1,
                },
                "flags": 0,
                "sequence": 0,
            },
            {
                "action": "CLEAR",
                "order": {
                    "side": "NO_ORDER_SIDE",
                    "price": "0",
                    "size": "0",
                    "order_id": 0,
                },
                "flags": 0,
                "sequence": 0,
            },
            {
                "action": "ADD",
                "order": {
                    "side": "SELL",
                    "price": "0.98039",
                    "size": "27.91",
                    "order_id": 2,
                },
                "flags": 0,
                "sequence": 0,
            },
            {
                "action": "ADD",
                "order": {
                    "side": "SELL",
                    "price": "0.97087",
                    "size": "14.43",
                    "order_id": 3,
                },
                "flags": 0,
                "sequence": 0,
            },
        ]
        deltas = OrderBookDeltas(
            instrument_id=TestIdStubs.audusd_id(),
            deltas=[OrderBookDelta.from_dict({**kw, **d}) for d in deltas],
        )

        # Act
        serialized = ArrowSerializer.serialize(deltas)
        deserialized = ArrowSerializer.deserialize(data_cls=OrderBookDeltas, batch=serialized)

        # Assert
        # assert deserialized == deltas.deltas # TODO - rust vs python types
        self.catalog.write_data(deserialized)
        assert [d.action for d in deserialized] == [
            BookAction.ADD,
            BookAction.CLEAR,
            BookAction.ADD,
            BookAction.ADD,
        ]

    def test_serialize_and_deserialize_component_state_changed(self):
        # Arrange
        event = TestEventStubs.component_state_changed()

        # Act
        serialized = ArrowSerializer.serialize(event)
        [deserialized] = ArrowSerializer.deserialize(
            data_cls=ComponentStateChanged,
            batch=serialized,
        )

        # Assert
        assert deserialized == event

        self.catalog.write_data([event])

    def test_serialize_and_deserialize_trading_state_changed(self):
        # Arrange
        event = TestEventStubs.trading_state_changed()

        # Act
        serialized = ArrowSerializer.serialize(event)
        [deserialized] = ArrowSerializer.deserialize(data_cls=TradingStateChanged, batch=serialized)

        # Assert
        assert deserialized == event

        self.catalog.write_data([event])

    @pytest.mark.parametrize(
        "event",
        [
            TestEventStubs.cash_account_state(),
            TestEventStubs.margin_account_state(),
        ],
    )
    def test_serialize_and_deserialize_account_state(self, event):
        # Arrange, Act
        serialized = ArrowSerializer.serialize(event, data_cls=AccountState)
        [deserialized] = ArrowSerializer.deserialize(data_cls=AccountState, batch=serialized)

        # Assert
        assert deserialized == event

        self.catalog.write_data([event])

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
        # Arrange, Act, Assert
        event = event_func(order=self.order_accepted)
        assert self._test_serialization(obj=event)

    @pytest.mark.parametrize(
        "event_func",
        [
            TestEventStubs.order_filled,
        ],
    )
    def test_serialize_and_deserialize_order_events_filled(self, event_func):
        # Arrange, Act, Assert
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
            TestInstrumentProvider.equity(),
            TestInstrumentProvider.future(),
            TestInstrumentProvider.aapl_option(),
        ],
    )
    def test_serialize_and_deserialize_instruments(self, instrument):
        serialized = ArrowSerializer.serialize(instrument)
        assert serialized
        deserialized = ArrowSerializer.deserialize(data_cls=type(instrument), batch=serialized)

        # Assert
        assert deserialized == [instrument]
        self.catalog.write_data([instrument])
        df = self.catalog.instruments()
        assert len(df) == 1

    @pytest.mark.parametrize("obj", nautilus_objects())
    def test_serialize_and_deserialize_all(self, obj):
        # Arrange, Act, Assert
        assert self._test_serialization(obj)

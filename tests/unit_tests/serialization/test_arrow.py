# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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
from nautilus_trader.common.messages import ShutdownSystem
from nautilus_trader.common.messages import TradingStateChanged
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.currencies import USDT
from nautilus_trader.model.data import OrderBookDelta
from nautilus_trader.model.data import OrderBookDeltas
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import BookAction
from nautilus_trader.model.enums import ContingencyType
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.enums import TriggerType
from nautilus_trader.model.events import AccountState
from nautilus_trader.model.events import OrderAccepted
from nautilus_trader.model.events import OrderCanceled
from nautilus_trader.model.events import OrderCancelRejected
from nautilus_trader.model.events import OrderDenied
from nautilus_trader.model.events import OrderEmulated
from nautilus_trader.model.events import OrderExpired
from nautilus_trader.model.events import OrderFilled
from nautilus_trader.model.events import OrderInitialized
from nautilus_trader.model.events import OrderModifyRejected
from nautilus_trader.model.events import OrderPendingCancel
from nautilus_trader.model.events import OrderPendingUpdate
from nautilus_trader.model.events import OrderRejected
from nautilus_trader.model.events import OrderReleased
from nautilus_trader.model.events import OrderSubmitted
from nautilus_trader.model.events import OrderTriggered
from nautilus_trader.model.events import OrderUpdated
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import ComponentId
from nautilus_trader.model.identifiers import ExecAlgorithmId
from nautilus_trader.model.identifiers import OrderListId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import AccountBalance
from nautilus_trader.model.objects import MarginBalance
from nautilus_trader.model.objects import Money
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


@pytest.mark.skipif(sys.platform == "win32", reason="Failing on windows")
class TestArrowSerializer:
    @pytest.fixture(autouse=True)
    def setup_method(self, tmp_path):
        # Fixture Setup
        self.trader_id = TestIdStubs.trader_id()
        self.strategy_id = TestIdStubs.strategy_id()
        self.account_id = TestIdStubs.account_id()
        self.venue = Venue("SIM")

        self.serializer = ArrowSerializer

        self.catalog = ParquetDataCatalog(path=str(tmp_path / "catalog"), fs_protocol="file")
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

    def test_serialize_and_deserialize_shutdown_system_commands(self):
        # Arrange
        command = ShutdownSystem(
            trader_id=TestIdStubs.trader_id(),
            component_id=ComponentId("Controller"),
            reason="Maintenance",
            command_id=UUID4(),
            ts_init=0,
        )

        # Act
        serialized = ArrowSerializer.serialize(command)
        [deserialized] = ArrowSerializer.deserialize(
            data_cls=ShutdownSystem,
            batch=serialized,
        )

        # Assert
        assert deserialized == command

        self.catalog.write_data([command])

    def test_serialize_and_deserialize_component_state_changed_events(self):
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

    def test_serialize_and_deserialize_trading_state_changed_events(self):
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
    def test_serialize_and_deserialize_account_state_events(self, event):
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

    def test_serialize_and_deserialize_account_state_with_base_currency_events(self):
        # Arrange
        event = AccountState(
            account_id=AccountId("SIM-000"),
            account_type=AccountType.MARGIN,
            base_currency=USD,
            reported=True,
            balances=[
                AccountBalance(
                    Money(1_525_000, USD),
                    Money(25_000, USD),
                    Money(1_500_000, USD),
                ),
            ],
            margins=[
                MarginBalance(
                    Money(5000, USD),
                    Money(20_000, USD),
                    AUDUSD_SIM.id,
                ),
            ],
            info={},
            event_id=UUID4(),
            ts_event=0,
            ts_init=1_000_000_000,
        )

        # Act
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(AccountState, batch=serialized)

        # Assert
        assert deserialized == [event]

    def test_serialize_and_deserialize_account_state_without_base_currency_events(self):
        # Arrange
        event = AccountState(
            account_id=AccountId("SIM-000"),
            account_type=AccountType.MARGIN,
            base_currency=None,
            reported=True,
            balances=[
                AccountBalance(
                    Money(10000, USDT),
                    Money(0, USDT),
                    Money(10000, USDT),
                ),
            ],
            margins=[],
            info={},
            event_id=UUID4(),
            ts_event=0,
            ts_init=1_000_000_000,
        )

        # Act
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(AccountState, serialized)

        # Assert
        assert deserialized == [event]

    def test_serialize_and_deserialize_account_state_with_margin_none_instrument_id(self):
        # Arrange
        event = AccountState(
            account_id=AccountId("BINANCE-001"),
            account_type=AccountType.MARGIN,
            base_currency=None,
            reported=True,
            balances=[
                AccountBalance(
                    Money(10000, USDT),
                    Money(100, USDT),
                    Money(9900, USDT),
                ),
            ],
            margins=[
                MarginBalance(
                    Money(500, USDT),
                    Money(250, USDT),
                    instrument_id=None,
                ),
            ],
            info={},
            event_id=UUID4(),
            ts_event=0,
            ts_init=1_000_000_000,
        )

        # Act
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(AccountState, serialized)

        # Assert
        assert deserialized == [event]

    def test_serialize_and_deserialize_market_order_initialized_events(self):
        # Arrange
        event = OrderInitialized(
            self.trader_id,
            self.strategy_id,
            AUDUSD_SIM.id,
            ClientOrderId("O-123456"),
            OrderSide.SELL,
            OrderType.MARKET,
            Quantity(100_000, precision=0),
            TimeInForce.FOK,
            post_only=False,
            reduce_only=True,
            quote_quantity=False,
            options={},
            emulation_trigger=TriggerType.NO_TRIGGER,
            trigger_instrument_id=None,
            contingency_type=ContingencyType.OTO,
            order_list_id=OrderListId("1"),
            linked_order_ids=[ClientOrderId("O-123457"), ClientOrderId("O-123458")],
            parent_order_id=ClientOrderId("O-123455"),
            exec_algorithm_id=ExecAlgorithmId("VWAP"),
            exec_algorithm_params={"period": 60},
            exec_spawn_id=ClientOrderId("O-1"),
            tags=["ENTRY"],
            event_id=UUID4(),
            ts_init=0,
        )

        # Act
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(OrderInitialized, batch=serialized)

        # Assert
        assert deserialized == [event]

    def test_serialize_and_deserialize_limit_order_initialized_events(self):
        # Arrange
        options = {
            "expire_time_ns": 1_000_000_000,
            "price": "1.0010",
        }

        event = OrderInitialized(
            self.trader_id,
            self.strategy_id,
            AUDUSD_SIM.id,
            ClientOrderId("O-123456"),
            OrderSide.SELL,
            OrderType.LIMIT,
            Quantity(100_000, precision=0),
            TimeInForce.DAY,
            post_only=True,
            reduce_only=False,
            quote_quantity=True,
            options=options,
            emulation_trigger=TriggerType.NO_TRIGGER,
            trigger_instrument_id=None,
            contingency_type=ContingencyType.OTO,
            order_list_id=OrderListId("1"),
            linked_order_ids=[ClientOrderId("O-123457"), ClientOrderId("O-123458")],
            parent_order_id=ClientOrderId("O-123455"),
            exec_algorithm_id=ExecAlgorithmId("VWAP"),
            exec_algorithm_params={"period": 60},
            exec_spawn_id=ClientOrderId("O-1"),
            tags=None,
            event_id=UUID4(),
            ts_init=0,
        )

        # Act
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(OrderInitialized, batch=serialized)

        # Assert
        assert deserialized == [event]
        assert deserialized[0].options == options
        assert deserialized[0].linked_order_ids == event.linked_order_ids
        assert deserialized[0].exec_algorithm_params == event.exec_algorithm_params

    def test_serialize_and_deserialize_stop_market_order_initialized_events(self):
        # Arrange
        options = {
            "trigger_price": "1.0005",
        }

        event = OrderInitialized(
            self.trader_id,
            self.strategy_id,
            AUDUSD_SIM.id,
            ClientOrderId("O-123456"),
            OrderSide.SELL,
            OrderType.STOP_MARKET,
            Quantity(100_000, precision=0),
            TimeInForce.DAY,
            post_only=False,
            reduce_only=True,
            quote_quantity=False,
            options=options,
            emulation_trigger=TriggerType.NO_TRIGGER,
            trigger_instrument_id=None,
            contingency_type=ContingencyType.OTO,
            order_list_id=OrderListId("1"),
            linked_order_ids=[ClientOrderId("O-123457"), ClientOrderId("O-123458")],
            parent_order_id=ClientOrderId("O-123455"),
            exec_algorithm_id=ExecAlgorithmId("VWAP"),
            exec_algorithm_params={"period": 60},
            exec_spawn_id=ClientOrderId("O-1"),
            tags=None,
            event_id=UUID4(),
            ts_init=0,
        )

        # Act
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(OrderInitialized, batch=serialized)

        # Assert
        assert deserialized == [event]
        assert deserialized[0].options == options
        assert deserialized[0].linked_order_ids == event.linked_order_ids

    def test_serialize_and_deserialize_stop_limit_order_initialized_events(self):
        # Arrange
        options = {
            "expire_time_ns": 0,
            "price": "1.0005",
            "trigger_price": "1.0010",
        }

        event = OrderInitialized(
            self.trader_id,
            self.strategy_id,
            AUDUSD_SIM.id,
            ClientOrderId("O-123456"),
            OrderSide.SELL,
            OrderType.STOP_LIMIT,
            Quantity(100_000, precision=0),
            TimeInForce.DAY,
            post_only=True,
            reduce_only=True,
            quote_quantity=False,
            options=options,
            emulation_trigger=TriggerType.NO_TRIGGER,
            trigger_instrument_id=None,
            contingency_type=ContingencyType.OTO,
            order_list_id=OrderListId("1"),
            linked_order_ids=[ClientOrderId("O-123457"), ClientOrderId("O-123458")],
            parent_order_id=ClientOrderId("O-123455"),
            exec_algorithm_id=ExecAlgorithmId("VWAP"),
            exec_algorithm_params={"period": 60},
            exec_spawn_id=ClientOrderId("O-1"),
            tags=["entry", "bulk"],
            event_id=UUID4(),
            ts_init=0,
        )

        # Act
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(OrderInitialized, batch=serialized)

        # Assert
        assert deserialized == [event]
        assert deserialized[0].options == options
        assert deserialized[0].linked_order_ids == event.linked_order_ids
        assert deserialized[0].tags == ["entry", "bulk"]

    def test_serialize_and_deserialize_order_denied_events(self):
        # Arrange
        event = OrderDenied(
            self.trader_id,
            self.strategy_id,
            AUDUSD_SIM.id,
            ClientOrderId("O-123456"),
            "Exceeds MAX_NOTIONAL_PER_ORDER",
            UUID4(),
            0,
        )

        # Act
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(OrderDenied, batch=serialized)

        # Assert
        assert deserialized == [event]

    def test_serialize_and_deserialize_order_emulated_events(self):
        # Arrange
        event = OrderEmulated(
            self.trader_id,
            self.strategy_id,
            AUDUSD_SIM.id,
            ClientOrderId("O-123456"),
            UUID4(),
            0,
        )

        # Act
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(OrderEmulated, batch=serialized)

        # Assert
        assert deserialized == [event]

    def test_serialize_and_deserialize_order_released_events(self):
        # Arrange
        event = OrderReleased(
            self.trader_id,
            self.strategy_id,
            AUDUSD_SIM.id,
            ClientOrderId("O-123456"),
            Price.from_str("1.00000"),
            UUID4(),
            0,
        )

        # Act
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(OrderReleased, batch=serialized)

        # Assert
        assert deserialized == [event]

    def test_serialize_and_deserialize_order_submitted_events(self):
        # Arrange
        event = OrderSubmitted(
            self.trader_id,
            self.strategy_id,
            AUDUSD_SIM.id,
            ClientOrderId("O-123456"),
            self.account_id,
            UUID4(),
            0,
            0,
        )

        # Act
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(OrderSubmitted, batch=serialized)

        # Assert
        assert deserialized == [event]

    def test_serialize_and_deserialize_order_accepted_events(self):
        # Arrange
        event = OrderAccepted(
            self.trader_id,
            self.strategy_id,
            AUDUSD_SIM.id,
            ClientOrderId("O-123456"),
            VenueOrderId("B-123456"),
            self.account_id,
            UUID4(),
            0,
            0,
        )

        # Act
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(OrderAccepted, batch=serialized)

        # Assert
        assert deserialized == [event]

    def test_serialize_and_deserialize_order_rejected_events(self):
        # Arrange
        event = OrderRejected(
            self.trader_id,
            self.strategy_id,
            AUDUSD_SIM.id,
            ClientOrderId("O-123456"),
            self.account_id,
            "ORDER_ID_INVALID",
            UUID4(),
            0,
            0,
        )

        # Act
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(OrderRejected, batch=serialized)

        # Assert
        assert deserialized == [event]

    def test_serialize_and_deserialize_order_pending_cancel_events(self):
        # Arrange
        event = OrderPendingCancel(
            self.trader_id,
            self.strategy_id,
            AUDUSD_SIM.id,
            ClientOrderId("O-123456"),
            VenueOrderId("1"),
            self.account_id,
            UUID4(),
            0,
            0,
        )

        # Act
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(OrderPendingCancel, batch=serialized)

        # Assert
        assert deserialized == [event]

    def test_serialize_and_deserialize_order_pending_replace_events(self):
        # Arrange
        event = OrderPendingUpdate(
            self.trader_id,
            self.strategy_id,
            AUDUSD_SIM.id,
            ClientOrderId("O-123456"),
            VenueOrderId("1"),
            self.account_id,
            UUID4(),
            0,
            0,
        )

        # Act
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(OrderPendingUpdate, batch=serialized)

        # Assert
        assert deserialized == [event]

    def test_serialize_and_deserialize_order_canceled_events(self):
        # Arrange
        event = OrderCanceled(
            self.trader_id,
            self.strategy_id,
            AUDUSD_SIM.id,
            ClientOrderId("O-123456"),
            VenueOrderId("1"),
            self.account_id,
            UUID4(),
            0,
            0,
        )

        # Act
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(OrderCanceled, batch=serialized)

        # Assert
        assert deserialized == [event]

    def test_serialize_and_deserialize_order_update_reject_events(self):
        # Arrange
        event = OrderModifyRejected(
            self.trader_id,
            self.strategy_id,
            AUDUSD_SIM.id,
            ClientOrderId("O-123456"),
            VenueOrderId("1"),
            self.account_id,
            "ORDER_DOES_NOT_EXIST",
            UUID4(),
            0,
            0,
        )

        # Act
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(OrderModifyRejected, batch=serialized)

        # Assert
        assert deserialized == [event]

    def test_serialize_and_deserialize_order_cancel_reject_events(self):
        # Arrange
        event = OrderCancelRejected(
            self.trader_id,
            self.strategy_id,
            AUDUSD_SIM.id,
            ClientOrderId("O-123456"),
            VenueOrderId("1"),
            self.account_id,
            "ORDER_DOES_NOT_EXIST",
            UUID4(),
            0,
            0,
        )

        # Act
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(OrderCancelRejected, batch=serialized)

        # Assert
        assert deserialized == [event]

    def test_serialize_and_deserialize_order_modify_events(self):
        # Arrange
        event = OrderUpdated(
            self.trader_id,
            self.strategy_id,
            AUDUSD_SIM.id,
            ClientOrderId("O-123456"),
            VenueOrderId("1"),
            self.account_id,
            Quantity(100_000, precision=0),
            Price(0.80010, precision=5),
            Price(0.80050, precision=5),
            UUID4(),
            0,
            0,
        )

        # Act
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(OrderUpdated, batch=serialized)

        # Assert
        assert deserialized == [event]

    def test_serialize_and_deserialize_order_expired_events(self):
        # Arrange
        event = OrderExpired(
            self.trader_id,
            self.strategy_id,
            AUDUSD_SIM.id,
            ClientOrderId("O-123456"),
            VenueOrderId("1"),
            self.account_id,
            UUID4(),
            0,
            0,
        )

        # Act
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(OrderExpired, batch=serialized)

        # Assert
        assert deserialized == [event]

    def test_serialize_and_deserialize_order_triggered_events(self):
        # Arrange
        event = OrderTriggered(
            self.trader_id,
            self.strategy_id,
            AUDUSD_SIM.id,
            ClientOrderId("O-123456"),
            VenueOrderId("1"),
            self.account_id,
            UUID4(),
            0,
            0,
        )

        # Act
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(OrderTriggered, batch=serialized)

        # Assert
        assert deserialized == [event]

    def test_serialize_and_deserialize_order_partially_filled_events(self):
        # Arrange
        event = OrderFilled(
            self.trader_id,
            self.strategy_id,
            AUDUSD_SIM.id,
            ClientOrderId("O-123456"),
            VenueOrderId("1"),
            self.account_id,
            TradeId("E123456"),
            PositionId("T123456"),
            OrderSide.SELL,
            OrderType.MARKET,
            Quantity(50_000, precision=0),
            Price(1.00000, precision=5),
            AUDUSD_SIM.quote_currency,
            Money(0, USD),
            LiquiditySide.MAKER,
            UUID4(),
            0,
            0,
        )

        # Act
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(OrderFilled, batch=serialized)

        # Assert
        assert deserialized == [event]

    def test_serialize_and_deserialize_order_filled_events(self):
        # Arrange
        event = OrderFilled(
            self.trader_id,
            self.strategy_id,
            AUDUSD_SIM.id,
            ClientOrderId("O-123456"),
            VenueOrderId("1"),
            self.account_id,
            TradeId("E123456"),
            PositionId("T123456"),
            OrderSide.SELL,
            OrderType.MARKET,
            Quantity(100_000, precision=0),
            Price(1.00000, precision=5),
            AUDUSD_SIM.quote_currency,
            Money(0, USD),
            LiquiditySide.TAKER,
            UUID4(),
            0,
            0,
        )

        # Act
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(OrderFilled, batch=serialized)

        # Assert
        assert deserialized == [event]

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
            TestInstrumentProvider.audusd_cfd(),
            TestInstrumentProvider.xbtusd_bitmex(),
            TestInstrumentProvider.btcusdt_future_binance(),
            TestInstrumentProvider.btcusdt_binance(),
            TestInstrumentProvider.equity(),
            TestInstrumentProvider.future(),
            TestInstrumentProvider.aapl_option(),
            TestInstrumentProvider.betting_instrument(),
            TestInstrumentProvider.binary_option(),
            TestInstrumentProvider.crypto_option(),
            TestInstrumentProvider.futures_spread(),
            TestInstrumentProvider.option_spread(),
            TestInstrumentProvider.commodity(),
            TestInstrumentProvider.index_instrument(),
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
        try:
            assert self._test_serialization(obj)
        except NotImplementedError as e:
            print(e)

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
import sys
from base64 import b64encode

import pytest

from nautilus_trader.backtest.data_loader import DataCatalog
from nautilus_trader.backtest.data_loader import class_to_filename
from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.factories import OrderFactory
from nautilus_trader.core.uuid import uuid4
from nautilus_trader.model.c_enums.book_level import BookLevel
from nautilus_trader.model.c_enums.delta_type import DeltaType
from nautilus_trader.model.commands.trading import CancelOrder
from nautilus_trader.model.commands.trading import SubmitBracketOrder
from nautilus_trader.model.commands.trading import SubmitOrder
from nautilus_trader.model.commands.trading import UpdateOrder
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.currencies import USDT
from nautilus_trader.model.data.tick import TradeTick
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.events.account import AccountState
from nautilus_trader.model.events.order import OrderAccepted
from nautilus_trader.model.events.order import OrderCanceled
from nautilus_trader.model.events.order import OrderCancelRejected
from nautilus_trader.model.events.order import OrderDenied
from nautilus_trader.model.events.order import OrderExpired
from nautilus_trader.model.events.order import OrderFilled
from nautilus_trader.model.events.order import OrderInitialized
from nautilus_trader.model.events.order import OrderPendingCancel
from nautilus_trader.model.events.order import OrderPendingUpdate
from nautilus_trader.model.events.order import OrderRejected
from nautilus_trader.model.events.order import OrderSubmitted
from nautilus_trader.model.events.order import OrderTriggered
from nautilus_trader.model.events.order import OrderUpdated
from nautilus_trader.model.events.order import OrderUpdateRejected
from nautilus_trader.model.events.position import PositionChanged
from nautilus_trader.model.events.position import PositionClosed
from nautilus_trader.model.events.position import PositionOpened
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import ExecutionId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import AccountBalance
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orderbook.data import OrderBookDelta
from nautilus_trader.model.orderbook.data import OrderBookDeltas
from nautilus_trader.model.orderbook.data import OrderBookSnapshot
from nautilus_trader.model.orders.limit import LimitOrder
from nautilus_trader.model.orders.stop_limit import StopLimitOrder
from nautilus_trader.model.orders.stop_market import StopMarketOrder
from nautilus_trader.model.orders.unpacker import OrderUnpacker
from nautilus_trader.model.position import Position
from nautilus_trader.serialization.arrow.core import _deserialize
from nautilus_trader.serialization.arrow.core import _serialize
from nautilus_trader.serialization.msgpack.serializer import MsgPackCommandSerializer
from nautilus_trader.serialization.msgpack.serializer import MsgPackEventSerializer
from nautilus_trader.serialization.msgpack.serializer import MsgPackInstrumentSerializer
from tests.test_kit.providers import TestInstrumentProvider
from tests.test_kit.stubs import UNIX_EPOCH
from tests.test_kit.stubs import TestStubs


AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")
ETHUSDT_BINANCE = TestInstrumentProvider.ethusdt_binance()


class TestMsgPackInstrumentSerializer:
    def setup(self):
        # Fixture Setup
        self.serializer = MsgPackInstrumentSerializer()

    def test_serialize_and_deserialize_fx_instrument(self):
        # Arrange, Act
        serialized = self.serializer.serialize(AUDUSD_SIM)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        assert deserialized == AUDUSD_SIM
        print(b64encode(serialized))
        print(deserialized)

    def test_serialize_and_deserialize_crypto_swap_instrument(self):
        # Arrange, Act
        serialized = self.serializer.serialize(ETHUSDT_BINANCE)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        assert deserialized == ETHUSDT_BINANCE
        print(b64encode(serialized))
        print(deserialized)

    def test_serialize_and_deserialize_crypto_instrument(self):
        # Arrange, Act
        serialized = self.serializer.serialize(ETHUSDT_BINANCE)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        assert deserialized == ETHUSDT_BINANCE
        print(b64encode(serialized))
        print(deserialized)


class TestOrderSerializer:
    def setup(self):
        # Fixture Setup
        self.unpacker = OrderUnpacker()
        self.trader_id = TestStubs.trader_id()
        self.strategy_id = TestStubs.strategy_id()

        self.order_factory = OrderFactory(
            trader_id=self.trader_id,
            strategy_id=self.strategy_id,
            clock=TestClock(),
        )

    def test_pack_and_unpack_market_orders(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity(100000, precision=0),
        )

        # Act
        packed = OrderInitialized.to_dict(order.last_event)
        unpacked = self.unpacker.unpack(packed)

        # Assert
        assert unpacked == order

    def test_pack_and_unpack_limit_orders(self):
        # Arrange
        order = self.order_factory.limit(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity(100000, precision=0),
            Price(1.00000, precision=5),
            TimeInForce.DAY,
        )

        # Act
        packed = OrderInitialized.to_dict(order.last_event)
        unpacked = self.unpacker.unpack(packed)

        # Assert
        assert unpacked == order

    def test_pack_and_unpack_limit_orders_with_expire_time(self):
        # Arrange
        order = LimitOrder(
            self.trader_id,
            self.strategy_id,
            AUDUSD_SIM.id,
            ClientOrderId("O-123456"),
            OrderSide.BUY,
            Quantity(100000, precision=0),
            price=Price(1.00000, precision=5),
            time_in_force=TimeInForce.GTD,
            expire_time=UNIX_EPOCH,
            init_id=uuid4(),
            ts_init=0,
        )

        # Act
        packed = OrderInitialized.to_dict(order.last_event)
        unpacked = self.unpacker.unpack(packed)

        # Assert
        assert unpacked == order

    def test_pack_and_unpack_stop_market_orders_with_expire_time(self):
        # Arrange
        order = StopMarketOrder(
            self.trader_id,
            self.strategy_id,
            AUDUSD_SIM.id,
            ClientOrderId("O-123456"),
            OrderSide.BUY,
            Quantity(100000, precision=0),
            price=Price(1.00000, precision=5),
            time_in_force=TimeInForce.GTD,
            expire_time=UNIX_EPOCH,
            init_id=uuid4(),
            ts_init=0,
        )

        # Act
        packed = OrderInitialized.to_dict(order.last_event)
        unpacked = self.unpacker.unpack(packed)

        # Assert
        assert unpacked == order

    def test_pack_and_unpack_stop_limit_orders(self):
        # Arrange
        order = StopLimitOrder(
            self.trader_id,
            self.strategy_id,
            AUDUSD_SIM.id,
            ClientOrderId("O-123456"),
            OrderSide.BUY,
            Quantity(100000, precision=0),
            price=Price(1.00000, precision=5),
            trigger=Price(1.00010, precision=5),
            time_in_force=TimeInForce.GTC,
            expire_time=None,
            init_id=uuid4(),
            ts_init=0,
        )

        # Act
        packed = OrderInitialized.to_dict(order.last_event)
        unpacked = self.unpacker.unpack(packed)

        # Assert
        assert unpacked == order

    def test_pack_and_unpack_stop_limit_orders_with_expire_time(self):
        # Arrange
        order = StopLimitOrder(
            self.trader_id,
            self.strategy_id,
            AUDUSD_SIM.id,
            ClientOrderId("O-123456"),
            OrderSide.BUY,
            Quantity(100000, precision=0),
            price=Price(1.00000, precision=5),
            trigger=Price(1.00010, precision=5),
            time_in_force=TimeInForce.GTD,
            expire_time=UNIX_EPOCH,
            init_id=uuid4(),
            ts_init=0,
        )

        # Act
        packed = OrderInitialized.to_dict(order.last_event)
        unpacked = self.unpacker.unpack(packed)

        # Assert
        assert unpacked == order


class TestMsgPackCommandSerializer:
    def setup(self):
        # Fixture Setup
        self.venue = Venue("SIM")
        self.trader_id = TestStubs.trader_id()
        self.strategy_id = TestStubs.strategy_id()
        self.account_id = TestStubs.account_id()
        self.serializer = MsgPackCommandSerializer()

        self.order_factory = OrderFactory(
            trader_id=self.trader_id,
            strategy_id=self.strategy_id,
            clock=TestClock(),
        )

    def test_serialize_and_deserialize_submit_order_commands(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity(100000, precision=0),
        )

        command = SubmitOrder(
            self.trader_id,
            StrategyId("SCALPER-001"),
            PositionId("P-123456"),
            order,
            uuid4(),
            0,
        )

        # Act
        serialized = self.serializer.serialize(command)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        assert deserialized == command
        assert deserialized.order == order
        print(command)
        print(len(serialized))
        print(serialized)
        print(b64encode(serialized))

    def test_serialize_and_deserialize_submit_bracket_order_no_take_profit_commands(
        self,
    ):
        # Arrange
        entry_order = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity(100000, precision=0),
        )

        bracket_order = self.order_factory.bracket(
            entry_order,
            stop_loss=Price(0.99900, precision=5),
            take_profit=Price(1.00100, precision=5),
        )

        command = SubmitBracketOrder(
            self.trader_id,
            StrategyId("SCALPER-001"),
            bracket_order,
            uuid4(),
            0,
        )

        # Act
        serialized = self.serializer.serialize(command)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        assert deserialized == command
        assert deserialized.bracket_order == bracket_order
        print(b64encode(serialized))
        print(command)

    def test_serialize_and_deserialize_submit_bracket_order_with_take_profit_commands(
        self,
    ):
        # Arrange
        entry_order = self.order_factory.limit(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity(100000, precision=0),
            Price(1.00000, precision=5),
        )

        bracket_order = self.order_factory.bracket(
            entry_order,
            stop_loss=Price(0.99900, precision=5),
            take_profit=Price(1.00010, precision=5),
        )

        command = SubmitBracketOrder(
            self.trader_id,
            StrategyId("SCALPER-001"),
            bracket_order,
            uuid4(),
            0,
        )

        # Act
        serialized = self.serializer.serialize(command)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        assert deserialized == command
        assert deserialized.bracket_order == bracket_order
        print(b64encode(serialized))
        print(command)

    def test_serialize_and_deserialize_amend_order_commands(self):
        # Arrange
        command = UpdateOrder(
            self.trader_id,
            StrategyId("SCALPER-001"),
            AUDUSD_SIM.id,
            ClientOrderId("O-123456"),
            VenueOrderId("001"),
            Quantity(100000, precision=0),
            Price(1.00001, precision=5),
            None,
            uuid4(),
            0,
        )

        # Act
        serialized = self.serializer.serialize(command)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        assert deserialized == command
        print(b64encode(serialized))
        print(command)

    def test_serialize_and_deserialize_cancel_order_commands(self):
        # Arrange
        command = CancelOrder(
            self.trader_id,
            StrategyId("SCALPER-001"),
            AUDUSD_SIM.id,
            ClientOrderId("O-123456"),
            VenueOrderId("001"),
            uuid4(),
            0,
        )

        # Act
        serialized = self.serializer.serialize(command)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        assert deserialized == command
        print(b64encode(serialized))
        print(command)


class TestMsgPackEventSerializer:
    def setup(self):
        # Fixture Setup
        self.trader_id = TestStubs.trader_id()
        self.strategy_id = TestStubs.strategy_id()
        self.account_id = TestStubs.account_id()

        self.order_factory = OrderFactory(
            trader_id=self.trader_id,
            strategy_id=self.strategy_id,
            clock=TestClock(),
        )
        self.serializer = MsgPackEventSerializer()

    def test_serialize_and_deserialize_account_state_with_base_currency_events(self):
        # Arrange
        event = AccountState(
            account_id=AccountId("SIM", "000"),
            account_type=AccountType.MARGIN,
            base_currency=USD,
            reported=True,
            balances=[AccountBalance(USD, Money(1525000, USD), Money(0, USD), Money(1525000, USD))],
            info={},
            event_id=uuid4(),
            ts_event=0,
            ts_init=1_000_000_000,
        )

        # Act
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        assert deserialized == event

    def test_serialize_and_deserialize_account_state_without_base_currency_events(self):
        # Arrange
        event = AccountState(
            account_id=AccountId("SIM", "000"),
            account_type=AccountType.MARGIN,
            base_currency=None,
            reported=True,
            balances=[
                AccountBalance(
                    USDT,
                    Money(10000, USDT),
                    Money(0, USDT),
                    Money(10000, USDT),
                )
            ],
            info={},
            event_id=uuid4(),
            ts_event=0,
            ts_init=1_000_000_000,
        )

        # Act
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        assert deserialized == event

    def test_serialize_and_deserialize_market_order_initialized_events(self):
        # Arrange
        event = OrderInitialized(
            self.trader_id,
            self.strategy_id,
            AUDUSD_SIM.id,
            ClientOrderId("O-123456"),
            OrderSide.SELL,
            OrderType.MARKET,
            Quantity(100000, precision=0),
            TimeInForce.FOK,
            uuid4(),
            0,
            options={},
        )

        # Act
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        assert deserialized == event

    def test_serialize_and_deserialize_limit_order_initialized_events(self):
        # Arrange
        options = {
            "ExpireTime": None,
            "Price": "1.0010",
            "PostOnly": True,
            "ReduceOnly": True,
            "Hidden": False,
        }

        event = OrderInitialized(
            self.trader_id,
            self.strategy_id,
            AUDUSD_SIM.id,
            ClientOrderId("O-123456"),
            OrderSide.SELL,
            OrderType.LIMIT,
            Quantity(100000, precision=0),
            TimeInForce.DAY,
            uuid4(),
            0,
            options=options,
        )

        # Act
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        assert deserialized == event
        assert deserialized.options == options

    def test_serialize_and_deserialize_stop_market_order_initialized_events(self):
        # Arrange
        options = {
            "ExpireTime": None,
            "Price": "1.0005",
            "ReduceOnly": False,
        }

        event = OrderInitialized(
            self.trader_id,
            self.strategy_id,
            AUDUSD_SIM.id,
            ClientOrderId("O-123456"),
            OrderSide.SELL,
            OrderType.STOP_MARKET,
            Quantity(100000, precision=0),
            TimeInForce.DAY,
            uuid4(),
            0,
            options=options,
        )

        # Act
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        assert deserialized == event
        assert deserialized.options == options

    def test_serialize_and_deserialize_stop_limit_order_initialized_events(self):
        # Arrange
        options = {
            "ExpireTime": None,
            "Price": "1.0005",
            "Trigger": "1.0010",
            "PostOnly": True,
            "ReduceOnly": False,
            "Hidden": False,
        }

        event = OrderInitialized(
            self.trader_id,
            self.strategy_id,
            AUDUSD_SIM.id,
            ClientOrderId("O-123456"),
            OrderSide.SELL,
            OrderType.STOP_LIMIT,
            Quantity(100000, precision=0),
            TimeInForce.DAY,
            uuid4(),
            0,
            options=options,
        )

        # Act
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        assert deserialized == event
        assert deserialized.options == options

    def test_serialize_and_deserialize_order_denied_events(self):
        # Arrange
        event = OrderDenied(
            self.trader_id,
            self.strategy_id,
            AUDUSD_SIM.id,
            ClientOrderId("O-123456"),
            "Exceeds MAX_NOTIONAL_PER_ORDER",
            uuid4(),
            0,
        )

        # Act
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        assert deserialized == event

    def test_serialize_and_deserialize_order_submitted_events(self):
        # Arrange
        event = OrderSubmitted(
            self.trader_id,
            self.strategy_id,
            self.account_id,
            AUDUSD_SIM.id,
            ClientOrderId("O-123456"),
            uuid4(),
            0,
            0,
        )

        # Act
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        assert deserialized == event

    def test_serialize_and_deserialize_order_accepted_events(self):
        # Arrange
        event = OrderAccepted(
            self.trader_id,
            self.strategy_id,
            self.account_id,
            AUDUSD_SIM.id,
            ClientOrderId("O-123456"),
            VenueOrderId("B-123456"),
            uuid4(),
            0,
            0,
        )

        # Act
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        assert deserialized == event

    def test_serialize_and_deserialize_order_rejected_events(self):
        # Arrange
        event = OrderRejected(
            self.trader_id,
            self.strategy_id,
            self.account_id,
            AUDUSD_SIM.id,
            ClientOrderId("O-123456"),
            "ORDER_ID_INVALID",
            uuid4(),
            0,
            0,
        )

        # Act
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        assert deserialized == event

    def test_serialize_and_deserialize_order_pending_cancel_events(self):
        # Arrange
        event = OrderPendingCancel(
            self.trader_id,
            self.strategy_id,
            self.account_id,
            AUDUSD_SIM.id,
            ClientOrderId("O-123456"),
            VenueOrderId("1"),
            uuid4(),
            0,
            0,
        )

        # Act
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        assert deserialized == event

    def test_serialize_and_deserialize_order_pending_replace_events(self):
        # Arrange
        event = OrderPendingUpdate(
            self.trader_id,
            self.strategy_id,
            self.account_id,
            AUDUSD_SIM.id,
            ClientOrderId("O-123456"),
            VenueOrderId("1"),
            uuid4(),
            0,
            0,
        )

        # Act
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        assert deserialized == event

    def test_serialize_and_deserialize_order_canceled_events(self):
        # Arrange
        event = OrderCanceled(
            self.trader_id,
            self.strategy_id,
            self.account_id,
            AUDUSD_SIM.id,
            ClientOrderId("O-123456"),
            VenueOrderId("1"),
            uuid4(),
            0,
            0,
        )

        # Act
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        assert deserialized == event

    def test_serialize_and_deserialize_order_update_reject_events(self):
        # Arrange
        event = OrderUpdateRejected(
            self.trader_id,
            self.strategy_id,
            self.account_id,
            AUDUSD_SIM.id,
            ClientOrderId("O-123456"),
            VenueOrderId("1"),
            "RESPONSE",
            "ORDER_DOES_NOT_EXIST",
            uuid4(),
            0,
            0,
        )

        # Act
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        assert deserialized == event

    def test_serialize_and_deserialize_order_cancel_reject_events(self):
        # Arrange
        event = OrderCancelRejected(
            self.trader_id,
            self.strategy_id,
            self.account_id,
            AUDUSD_SIM.id,
            ClientOrderId("O-123456"),
            VenueOrderId("1"),
            "RESPONSE",
            "ORDER_DOES_NOT_EXIST",
            uuid4(),
            0,
            0,
        )

        # Act
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        assert deserialized == event

    def test_serialize_and_deserialize_order_amended_events(self):
        # Arrange
        event = OrderUpdated(
            self.trader_id,
            self.strategy_id,
            self.account_id,
            AUDUSD_SIM.id,
            ClientOrderId("O-123456"),
            VenueOrderId("1"),
            Quantity(100000, precision=0),
            Price(0.80010, precision=5),
            Price(0.80050, precision=5),
            uuid4(),
            0,
            0,
        )

        # Act
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        assert deserialized == event

    def test_serialize_and_deserialize_order_expired_events(self):
        # Arrange
        event = OrderExpired(
            self.trader_id,
            self.strategy_id,
            self.account_id,
            AUDUSD_SIM.id,
            ClientOrderId("O-123456"),
            VenueOrderId("1"),
            uuid4(),
            0,
            0,
        )

        # Act
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        assert deserialized == event

    def test_serialize_and_deserialize_order_triggered_events(self):
        # Arrange
        event = OrderTriggered(
            self.trader_id,
            self.strategy_id,
            self.account_id,
            AUDUSD_SIM.id,
            ClientOrderId("O-123456"),
            VenueOrderId("1"),
            uuid4(),
            0,
            0,
        )

        # Act
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        assert deserialized == event

    def test_serialize_and_deserialize_order_partially_filled_events(self):
        # Arrange
        event = OrderFilled(
            self.trader_id,
            self.strategy_id,
            self.account_id,
            AUDUSD_SIM.id,
            ClientOrderId("O-123456"),
            VenueOrderId("1"),
            ExecutionId("E123456"),
            PositionId("T123456"),
            OrderSide.SELL,
            OrderType.MARKET,
            Quantity(50000, precision=0),
            Price(1.00000, precision=5),
            AUDUSD_SIM.quote_currency,
            Money(0, USD),
            LiquiditySide.MAKER,
            uuid4(),
            0,
            0,
        )

        # Act
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        assert deserialized == event

    def test_serialize_and_deserialize_order_filled_events(self):
        # Arrange
        event = OrderFilled(
            self.trader_id,
            self.strategy_id,
            self.account_id,
            AUDUSD_SIM.id,
            ClientOrderId("O-123456"),
            VenueOrderId("1"),
            ExecutionId("E123456"),
            PositionId("T123456"),
            OrderSide.SELL,
            OrderType.MARKET,
            Quantity(100000, precision=0),
            Price(1.00000, precision=5),
            AUDUSD_SIM.quote_currency,
            Money(0, USD),
            LiquiditySide.TAKER,
            uuid4(),
            0,
            0,
        )

        # Act
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        assert deserialized == event

    def test_serialize_and_deserialize_position_opened_events(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )

        fill = TestStubs.event_order_filled(
            order,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("1.00001"),
        )

        position = Position(instrument=AUDUSD_SIM, fill=fill)

        uuid = uuid4()
        event = PositionOpened.create(position, fill, uuid, 0)

        # Act
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        assert deserialized == event

    def test_serialize_and_deserialize_position_changed_events(self):
        # Arrange
        order1 = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )

        fill1 = TestStubs.event_order_filled(
            order1,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("1.00001"),
        )

        order2 = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(50000),
        )

        fill2 = TestStubs.event_order_filled(
            order2,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("1.00011"),
        )

        position = Position(instrument=AUDUSD_SIM, fill=fill1)
        position.apply(fill2)

        uuid = uuid4()
        event = PositionChanged.create(position, fill2, uuid, 0)

        # Act
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        assert deserialized == event

    def test_serialize_and_deserialize_position_closed_events(self):
        # Arrange
        order1 = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )

        fill1 = TestStubs.event_order_filled(
            order1,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("1.00001"),
        )

        order2 = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(100000),
        )

        fill2 = TestStubs.event_order_filled(
            order2,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("1.00011"),
        )

        position = Position(instrument=AUDUSD_SIM, fill=fill1)
        position.apply(fill2)

        uuid = uuid4()
        event = PositionClosed.create(position, fill2, uuid, 0)

        # Act
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        assert deserialized == event


@pytest.mark.skipif(sys.platform == "win32", reason="does not run on windows")
class TestParquetSerializer:
    def setup(self):
        self.catalog = DataCatalog(path="/", fs_protocol="memory")
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

    def test_serialize_and_deserialize_trade_tick(self):
        tick = TestStubs.trade_tick_5decimal()

        serialized = _serialize(tick)
        deserialized = _deserialize(cls=TradeTick, chunk=[serialized])

        # Assert
        assert deserialized == [tick]
        self.catalog._write_chunks([tick])

    def test_serialize_and_deserialize_order_book_delta(self):
        delta = OrderBookDelta(
            instrument_id=TestStubs.audusd_id(),
            level=BookLevel.L2,
            delta_type=DeltaType.CLEAR,
            order=None,
            ts_event=0,
            ts_init=0,
        )

        serialized = _serialize(delta)
        [deserialized] = _deserialize(cls=OrderBookDelta, chunk=serialized)

        # Assert
        expected = OrderBookDeltas(
            instrument_id=TestStubs.audusd_id(),
            level=BookLevel.L2,
            deltas=[delta],
            ts_event=0,
            ts_init=0,
        )
        assert deserialized == expected
        self.catalog._write_chunks([delta])

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

        serialized = _serialize(deltas)
        deserialized = _deserialize(cls=OrderBookDeltas, chunk=serialized)

        # Assert
        assert deserialized == [deltas]
        self.catalog._write_chunks([deltas])

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

        serialized = _serialize(deltas)
        [deserialized] = _deserialize(cls=OrderBookDeltas, chunk=serialized)

        # Assert
        assert deserialized == deltas
        self.catalog._write_chunks([deserialized])
        assert [d.type for d in deserialized.deltas] == [
            DeltaType.ADD,
            DeltaType.CLEAR,
            DeltaType.ADD,
            DeltaType.ADD,
        ]

    def test_serialize_and_deserialize_order_book_snapshot(self):
        book = TestStubs.order_book_snapshot()

        serialized = _serialize(book)
        deserialized = _deserialize(cls=OrderBookSnapshot, chunk=serialized)

        # Assert
        assert deserialized == [book]
        self.catalog._write_chunks([book])

    def test_serialize_and_deserialize_account_state(self):
        account = TestStubs.event_cash_account_state()

        serialized = _serialize(account)
        [deserialized] = _deserialize(cls=AccountState, chunk=serialized)

        # Assert
        assert deserialized == account

        self.catalog._write_chunks([account])

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

        serialized = _serialize(event)
        deserialized = _deserialize(cls=cls, chunk=serialized)

        # Assert
        assert deserialized == [event]
        self.catalog._write_chunks([event])
        df = self.catalog._query(class_to_filename(cls))
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

        serialized = _serialize(event)
        deserialized = _deserialize(cls=cls, chunk=serialized)

        # Assert
        assert deserialized == [event]
        self.catalog._write_chunks([event])
        df = self.catalog._query(class_to_filename(cls))
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

        serialized = _serialize(event)
        assert serialized
        # TODO (bm) - can't deserialize order filled right now
        # deserialized = _deserialize(cls=cls, chunk=serialized)

        # Assert
        # assert deserialized == [event]
        self.catalog._write_chunks([event])
        df = self.catalog._query(class_to_filename(cls))
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

        serialized = _serialize(event)
        assert serialized
        # TODO (bm) - can't deserialize positions right now
        # deserialized = _deserialize(cls=cls, chunk=serialized)

        # Assert
        # assert deserialized == [event]
        self.catalog._write_chunks([event])
        df = self.catalog._query(class_to_filename(cls))
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

        serialized = _serialize(event)
        assert serialized
        # TODO (bm) - can't deserialize positions right now
        # deserialized = _deserialize(cls=cls, chunk=serialized)

        # Assert
        # assert deserialized == [event]
        self.catalog._write_chunks([event])
        df = self.catalog._query(class_to_filename(cls))
        assert len(df) == 1

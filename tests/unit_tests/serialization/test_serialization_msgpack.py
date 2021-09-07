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

from base64 import b64encode

import msgpack
import pytest

from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.enums import ComponentState
from nautilus_trader.common.events.system import ComponentStateChanged
from nautilus_trader.common.factories import OrderFactory
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.model.commands.trading import CancelOrder
from nautilus_trader.model.commands.trading import ModifyOrder
from nautilus_trader.model.commands.trading import SubmitBracketOrder
from nautilus_trader.model.commands.trading import SubmitOrder
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.currencies import USDT
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
from nautilus_trader.model.events.order import OrderModifyRejected
from nautilus_trader.model.events.order import OrderPendingCancel
from nautilus_trader.model.events.order import OrderPendingUpdate
from nautilus_trader.model.events.order import OrderRejected
from nautilus_trader.model.events.order import OrderSubmitted
from nautilus_trader.model.events.order import OrderTriggered
from nautilus_trader.model.events.order import OrderUpdated
from nautilus_trader.model.events.position import PositionChanged
from nautilus_trader.model.events.position import PositionClosed
from nautilus_trader.model.events.position import PositionOpened
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import ComponentId
from nautilus_trader.model.identifiers import ExecutionId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import AccountBalance
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orders.limit import LimitOrder
from nautilus_trader.model.orders.stop_limit import StopLimitOrder
from nautilus_trader.model.orders.stop_market import StopMarketOrder
from nautilus_trader.model.orders.unpacker import OrderUnpacker
from nautilus_trader.model.position import Position
from nautilus_trader.serialization.msgpack.serializer import MsgPackSerializer
from tests.test_kit.providers import TestInstrumentProvider
from tests.test_kit.stubs import UNIX_EPOCH
from tests.test_kit.stubs import TestStubs


AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")
ETHUSDT_BINANCE = TestInstrumentProvider.ethusdt_binance()


class TestMsgPackSerializer:
    def setup(self):
        # Fixture Setup
        self.trader_id = TestStubs.trader_id()
        self.strategy_id = TestStubs.strategy_id()
        self.account_id = TestStubs.account_id()
        self.venue = Venue("SIM")

        self.unpacker = OrderUnpacker()
        self.order_factory = OrderFactory(
            trader_id=self.trader_id,
            strategy_id=self.strategy_id,
            clock=TestClock(),
        )

        self.serializer = MsgPackSerializer()

    def test_serialize_unknown_object_raises_runtime_error(self):
        # Arrange, Act
        with pytest.raises(RuntimeError):
            self.serializer.serialize({"type": "UNKNOWN"})

    def test_deserialize_unknown_object_raises_runtime_error(self):
        # Arrange, Act
        with pytest.raises(RuntimeError):
            self.serializer.deserialize(msgpack.packb({"type": "UNKNOWN"}))

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
            init_id=UUID4(),
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
            init_id=UUID4(),
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
            init_id=UUID4(),
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
            init_id=UUID4(),
            ts_init=0,
        )

        # Act
        packed = OrderInitialized.to_dict(order.last_event)
        unpacked = self.unpacker.unpack(packed)

        # Assert
        assert unpacked == order

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
            UUID4(),
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
            UUID4(),
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
            UUID4(),
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
        command = ModifyOrder(
            self.trader_id,
            StrategyId("SCALPER-001"),
            AUDUSD_SIM.id,
            ClientOrderId("O-123456"),
            VenueOrderId("001"),
            Quantity(100000, precision=0),
            Price(1.00001, precision=5),
            None,
            UUID4(),
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
            UUID4(),
            0,
        )

        # Act
        serialized = self.serializer.serialize(command)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        assert deserialized == command
        print(b64encode(serialized))
        print(command)

    def test_serialize_and_deserialize_component_state_changed_events(self):
        # Arrange
        event = ComponentStateChanged(
            trader_id=TestStubs.trader_id(),
            component_id=ComponentId("MyActor-001"),
            component_type="MyActor",
            state=ComponentState.RUNNING,
            config={"do_something": True},
            event_id=UUID4(),
            ts_event=0,
            ts_init=0,
        )

        # Act
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        assert deserialized == event

    def test_serialize_and_deserialize_account_state_with_base_currency_events(self):
        # Arrange
        event = AccountState(
            account_id=AccountId("SIM", "000"),
            account_type=AccountType.MARGIN,
            base_currency=USD,
            reported=True,
            balances=[AccountBalance(USD, Money(1525000, USD), Money(0, USD), Money(1525000, USD))],
            info={},
            event_id=UUID4(),
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
            event_id=UUID4(),
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
            options={},
            tags=None,
            event_id=UUID4(),
            ts_init=0,
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
            options=options,
            tags=None,
            event_id=UUID4(),
            ts_init=0,
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
            options=options,
            tags=None,
            event_id=UUID4(),
            ts_init=0,
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
            options=options,
            tags="entry,bulk",
            event_id=UUID4(),
            ts_init=0,
        )

        # Act
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        assert deserialized == event
        assert deserialized.options == options
        assert deserialized.tags == "entry,bulk"

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
            UUID4(),
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
            UUID4(),
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
            UUID4(),
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
            UUID4(),
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
            UUID4(),
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
            UUID4(),
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
        event = OrderModifyRejected(
            self.trader_id,
            self.strategy_id,
            self.account_id,
            AUDUSD_SIM.id,
            ClientOrderId("O-123456"),
            VenueOrderId("1"),
            "ORDER_DOES_NOT_EXIST",
            UUID4(),
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
            "ORDER_DOES_NOT_EXIST",
            UUID4(),
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
            UUID4(),
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
            UUID4(),
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
            UUID4(),
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
            UUID4(),
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
            UUID4(),
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

        uuid = UUID4()
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

        uuid = UUID4()
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

        uuid = UUID4()
        event = PositionClosed.create(position, fill2, uuid, 0)

        # Act
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        assert deserialized == event

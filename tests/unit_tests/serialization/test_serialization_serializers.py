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

from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.factories import OrderFactory
from nautilus_trader.core.uuid import uuid4
from nautilus_trader.model.commands import CancelOrder
from nautilus_trader.model.commands import SubmitBracketOrder
from nautilus_trader.model.commands import SubmitOrder
from nautilus_trader.model.commands import UpdateOrder
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.events import AccountState
from nautilus_trader.model.events import OrderAccepted
from nautilus_trader.model.events import OrderCancelRejected
from nautilus_trader.model.events import OrderCanceled
from nautilus_trader.model.events import OrderDenied
from nautilus_trader.model.events import OrderExpired
from nautilus_trader.model.events import OrderFilled
from nautilus_trader.model.events import OrderInitialized
from nautilus_trader.model.events import OrderInvalid
from nautilus_trader.model.events import OrderPendingCancel
from nautilus_trader.model.events import OrderPendingReplace
from nautilus_trader.model.events import OrderRejected
from nautilus_trader.model.events import OrderSubmitted
from nautilus_trader.model.events import OrderTriggered
from nautilus_trader.model.events import OrderUpdateRejected
from nautilus_trader.model.events import OrderUpdated
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
from nautilus_trader.model.orders.limit import LimitOrder
from nautilus_trader.model.orders.stop_limit import StopLimitOrder
from nautilus_trader.model.orders.stop_market import StopMarketOrder
from nautilus_trader.serialization.serializers import MsgPackCommandSerializer
from nautilus_trader.serialization.serializers import MsgPackEventSerializer
from nautilus_trader.serialization.serializers import MsgPackInstrumentSerializer
from nautilus_trader.serialization.serializers import MsgPackOrderSerializer
from tests.test_kit.providers import TestInstrumentProvider
from tests.test_kit.stubs import TestStubs
from tests.test_kit.stubs import UNIX_EPOCH


AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")
ETHUSDT_BINANCE = TestInstrumentProvider.ethusdt_binance()


class TestInstrumentSerializer:
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

    def test_serialize_and_deserialize_crypto_instrument(self):
        # Arrange, Act
        serialized = self.serializer.serialize(ETHUSDT_BINANCE)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        assert deserialized == ETHUSDT_BINANCE
        print(b64encode(serialized))
        print(deserialized)


class TestMsgPackOrderSerializer:
    def setup(self):
        # Fixture Setup
        self.serializer = MsgPackOrderSerializer()
        self.order_factory = OrderFactory(
            trader_id=TraderId("TESTER-000"),
            strategy_id=StrategyId("S-001"),
            clock=TestClock(),
        )

    def test_serialize_and_deserialize_market_orders(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity(100000, precision=0),
        )

        # Act
        serialized = self.serializer.serialize(order)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        assert deserialized == order
        print(b64encode(serialized))
        print(order)

    def test_serialize_and_deserialize_limit_orders(self):
        # Arrange
        order = self.order_factory.limit(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity(100000, precision=0),
            Price(1.00000, precision=5),
            TimeInForce.DAY,
        )

        # Act
        serialized = self.serializer.serialize(order)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        assert deserialized == order
        print(b64encode(serialized))
        print(order)

    def test_serialize_and_deserialize_limit_orders_with_expire_time(self):
        # Arrange
        order = LimitOrder(
            ClientOrderId("O-123456"),
            StrategyId("S-001"),
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity(100000, precision=0),
            price=Price(1.00000, precision=5),
            time_in_force=TimeInForce.GTD,
            expire_time=UNIX_EPOCH,
            init_id=uuid4(),
            timestamp_ns=0,
        )

        # Act
        serialized = self.serializer.serialize(order)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        assert deserialized == order
        print(b64encode(serialized))
        print(order)

    def test_serialize_and_deserialize_stop_market_orders_with_expire_time(self):
        # Arrange
        order = StopMarketOrder(
            ClientOrderId("O-123456"),
            StrategyId("S-001"),
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity(100000, precision=0),
            price=Price(1.00000, precision=5),
            time_in_force=TimeInForce.GTD,
            expire_time=UNIX_EPOCH,
            init_id=uuid4(),
            timestamp_ns=0,
        )

        # Act
        serialized = self.serializer.serialize(order)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        assert deserialized == order
        print(b64encode(serialized))
        print(order)

    def test_serialize_and_deserialize_stop_limit_orders(self):
        # Arrange
        order = StopLimitOrder(
            ClientOrderId("O-123456"),
            StrategyId("S-001"),
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity(100000, precision=0),
            price=Price(1.00000, precision=5),
            trigger=Price(1.00010, precision=5),
            time_in_force=TimeInForce.GTC,
            expire_time=None,
            init_id=uuid4(),
            timestamp_ns=0,
        )

        # Act
        serialized = self.serializer.serialize(order)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        assert deserialized == order
        print(b64encode(serialized))
        print(order)

    def test_serialize_and_deserialize_stop_limit_orders_with_expire_time(self):
        # Arrange
        order = StopLimitOrder(
            ClientOrderId("O-123456"),
            StrategyId("S-001"),
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity(100000, precision=0),
            price=Price(1.00000, precision=5),
            trigger=Price(1.00010, precision=5),
            time_in_force=TimeInForce.GTD,
            expire_time=UNIX_EPOCH,
            init_id=uuid4(),
            timestamp_ns=0,
        )

        # Act
        serialized = self.serializer.serialize(order)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        assert deserialized == order
        print(b64encode(serialized))
        print(order)


class TestMsgPackCommandSerializer:
    def setup(self):
        # Fixture Setup
        self.venue = Venue("SIM")
        self.trader_id = TestStubs.trader_id()
        self.account_id = TestStubs.account_id()
        self.serializer = MsgPackCommandSerializer()
        self.order_factory = OrderFactory(
            trader_id=self.trader_id,
            strategy_id=StrategyId("S-001"),
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
        self.account_id = TestStubs.account_id()
        self.serializer = MsgPackEventSerializer()

    def test_serialize_and_deserialize_account_state_events(self):
        # Arrange
        event = AccountState(
            account_id=AccountId("SIM", "000"),
            account_type=AccountType.MARGIN,
            base_currency=USD,
            reported=True,
            balances=[
                AccountBalance(
                    USD, Money(1525000, USD), Money(0, USD), Money(1525000, USD)
                )
            ],
            info={},
            event_id=uuid4(),
            ts_updated_ns=0,
            timestamp_ns=1_000_000_000,
        )

        # Act
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(serialized)
        print(deserialized)

        # Assert
        assert deserialized == event

    def test_serialize_and_deserialize_market_order_initialized_events(self):
        # Arrange
        event = OrderInitialized(
            ClientOrderId("O-123456"),
            StrategyId("S-001"),
            AUDUSD_SIM.id,
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
            ClientOrderId("O-123456"),
            StrategyId("S-001"),
            AUDUSD_SIM.id,
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
            ClientOrderId("O-123456"),
            StrategyId("S-001"),
            AUDUSD_SIM.id,
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
            ClientOrderId("O-123456"),
            StrategyId("S-001"),
            AUDUSD_SIM.id,
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

    def test_serialize_and_deserialize_order_invalid_events(self):
        # Arrange
        event = OrderInvalid(
            ClientOrderId("O-123456"),
            "VenueOrderId already exists",
            uuid4(),
            0,
        )

        # Act
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        assert deserialized == event

    def test_serialize_and_deserialize_order_denied_events(self):
        # Arrange
        event = OrderDenied(
            ClientOrderId("O-123456"),
            "Exceeds risk for FX",
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
            self.account_id,
            ClientOrderId("O-123456"),
            0,
            uuid4(),
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
            self.account_id,
            ClientOrderId("O-123456"),
            VenueOrderId("B-123456"),
            0,
            uuid4(),
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
            self.account_id,
            ClientOrderId("O-123456"),
            "ORDER_ID_INVALID",
            0,
            uuid4(),
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
            self.account_id,
            ClientOrderId("O-123456"),
            VenueOrderId("1"),
            0,
            uuid4(),
            0,
        )

        # Act
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        assert deserialized == event

    def test_serialize_and_deserialize_order_pending_replace_events(self):
        # Arrange
        event = OrderPendingReplace(
            self.account_id,
            ClientOrderId("O-123456"),
            VenueOrderId("1"),
            0,
            uuid4(),
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
            self.account_id,
            ClientOrderId("O-123456"),
            VenueOrderId("1"),
            0,
            uuid4(),
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
            self.account_id,
            ClientOrderId("O-123456"),
            VenueOrderId("1"),
            "RESPONSE",
            "ORDER_DOES_NOT_EXIST",
            0,
            uuid4(),
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
            self.account_id,
            ClientOrderId("O-123456"),
            VenueOrderId("1"),
            "RESPONSE",
            "ORDER_DOES_NOT_EXIST",
            0,
            uuid4(),
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
            self.account_id,
            ClientOrderId("O-123456"),
            VenueOrderId("1"),
            Quantity(100000, precision=0),
            Price(0.80010, precision=5),
            0,
            uuid4(),
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
            self.account_id,
            ClientOrderId("O-123456"),
            VenueOrderId("1"),
            0,
            uuid4(),
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
            self.account_id,
            ClientOrderId("O-123456"),
            VenueOrderId("1"),
            0,
            uuid4(),
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
            self.account_id,
            ClientOrderId("O-123456"),
            VenueOrderId("1"),
            ExecutionId("E123456"),
            PositionId("T123456"),
            StrategyId("S-001"),
            AUDUSD_SIM.id,
            OrderSide.SELL,
            Quantity(50000, precision=0),
            Price(1.00000, precision=5),
            AUDUSD_SIM.quote_currency,
            Money(0, USD),
            LiquiditySide.MAKER,
            0,
            uuid4(),
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
            self.account_id,
            ClientOrderId("O-123456"),
            VenueOrderId("1"),
            ExecutionId("E123456"),
            PositionId("T123456"),
            StrategyId("S-001"),
            AUDUSD_SIM.id,
            OrderSide.SELL,
            Quantity(100000, precision=0),
            Price(1.00000, precision=5),
            AUDUSD_SIM.quote_currency,
            Money(0, USD),
            LiquiditySide.TAKER,
            0,
            uuid4(),
            0,
        )

        # Act
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        assert deserialized == event

# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
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

import unittest

from nautilus_trader.core.decimal import Decimal64
from nautilus_trader.core.uuid import uuid4
from nautilus_trader.core.types import ValidString, Label
from nautilus_trader.model.enums import OrderSide, OrderType, OrderState, OrderPurpose
from nautilus_trader.model.enums import TimeInForce, Currency
from nautilus_trader.model.events import OrderInitialized, OrderSubmitted, OrderAccepted
from nautilus_trader.model.events import OrderRejected, OrderWorking, OrderExpired
from nautilus_trader.model.events import OrderModified, OrderCancelled, OrderCancelReject
from nautilus_trader.model.events import OrderFilled, OrderPartiallyFilled
from nautilus_trader.model.identifiers import IdTag, OrderId, OrderIdBroker
from nautilus_trader.model.identifiers import BracketOrderId, ExecutionId, PositionIdBroker
from nautilus_trader.model.objects import Quantity, Price
from nautilus_trader.model.order import Order
from nautilus_trader.common.uuid import TestUUIDFactory
from nautilus_trader.common.factories import OrderFactory
from nautilus_trader.common.clock import TestClock
from tests.test_kit.stubs import TestStubs, UNIX_EPOCH


AUDUSD_FXCM = TestStubs.symbol_audusd_fxcm()
GBPUSD_FXCM = TestStubs.symbol_gbpusd_fxcm()


class OrderTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        self.account_id = TestStubs.account_id()
        self.order_factory = OrderFactory(
            id_tag_trader=IdTag("001"),
            id_tag_strategy=IdTag("001"),
            clock=TestClock(),
            uuid_factory=TestUUIDFactory())

    def test_market_order_with_quantity_zero_raises_exception(self):
        # Arrange
        # Act
        self.assertRaises(
            ValueError,
            Order,
            OrderId("O-123456"),
            AUDUSD_FXCM,
            OrderSide.BUY,
            OrderType.MARKET,
            Quantity(),
            uuid4(),
            UNIX_EPOCH)

    def test_priced_order_with_GTD_time_in_force_and_expire_time_none_raises_exception(  # noqa: N802
        self
    ):
        # Arrange
        # Act
        self.assertRaises(
            ValueError,
            Order,
            OrderId("O-123456"),
            AUDUSD_FXCM,
            OrderSide.BUY,
            OrderType.LIMIT,
            Quantity(100000),
            uuid4(),
            UNIX_EPOCH,
            price=Price(1.00000, 5),
            time_in_force=TimeInForce.GTD,
            expire_time=None)

    def test_market_order_with_price_input_raises_exception(self):
        # Arrange
        # Act
        self.assertRaises(
            ValueError,
            Order,
            OrderId("O-123456"),
            AUDUSD_FXCM,
            OrderSide.BUY,
            OrderType.MARKET,
            Quantity(100000),
            uuid4(),
            UNIX_EPOCH,
            price=Price(1.00000, 5))

    def test_stop_order_with_no_price_input_raises_exception(self):
        # Arrange
        # Act
        self.assertRaises(
            ValueError,
            Order,
            OrderId("O-123456"),
            AUDUSD_FXCM,
            OrderSide.BUY,
            OrderType.STOP,
            Quantity(100000),
            uuid4(),
            UNIX_EPOCH)

    def test_stop_order_with_zero_price_input_raises_exception(self):
        # Arrange
        # Act
        self.assertRaises(
            ValueError,
            Order,
            OrderId("O-123456"),
            AUDUSD_FXCM,
            OrderSide.BUY,
            OrderType.STOP,
            Quantity(100000),
            uuid4(),
            UNIX_EPOCH,
            price=None)

    def test_can_reset_order_factory(self):
        # Arrange
        self.order_factory.limit(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000),
            Price(1.00000, 5))

        # Act
        self.order_factory.reset()

        order2 = self.order_factory.limit(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000),
            Price(1.00000, 5))

        self.assertEqual(OrderId("O-19700101-000000-001-001-1"), order2.id)

    def test_limit_order_can_create_expected_decimal_price(self):
        # Arrange
        # Act
        order1 = self.order_factory.limit(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000),
            Price(1.00000, 5))

        order2 = self.order_factory.limit(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000),
            Price(1.00000, 5))

        order3 = self.order_factory.limit(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000),
            Price(1.00000, 5))

        order4 = self.order_factory.limit(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000),
            Price(1.00001, 5))

        # Assert
        self.assertEqual(Price(1.00000, 5), order1.price)
        self.assertEqual(Price(1.00000, 5), order2.price)
        self.assertEqual(Price(1.00000, 5), order3.price)
        self.assertEqual(Price(1.00001, 5), order4.price)

    def test_can_initialize_buy_market_order(self):
        # Arrange
        # Act
        order = self.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000),)

        # Assert
        self.assertEqual(OrderType.MARKET, order.type)
        self.assertEqual(OrderState.INITIALIZED, order.state)
        self.assertEqual(1, order.event_count)
        self.assertTrue(isinstance(order.last_event, OrderInitialized))
        self.assertFalse(order.is_working)
        self.assertFalse(order.is_completed)
        self.assertTrue(order.is_buy)
        self.assertFalse(order.is_sell)
        self.assertEqual(None, order.filled_timestamp)
        self.assertEqual(UNIX_EPOCH, order.last_event_time())

    def test_can_initialize_sell_market_order(self):
        # Arrange
        # Act
        order = self.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.SELL,
            Quantity(100000),)

        # Assert
        self.assertEqual(OrderType.MARKET, order.type)
        self.assertEqual(OrderState.INITIALIZED, order.state)
        self.assertEqual(1, order.event_count)
        self.assertTrue(isinstance(order.last_event, OrderInitialized))
        self.assertFalse(order.is_working)
        self.assertFalse(order.is_completed)
        self.assertFalse(order.is_buy)
        self.assertTrue(order.is_sell)
        self.assertEqual(None, order.filled_timestamp)

    def test_order_str_and_repr(self):
        # Arrange
        # Act
        order = self.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))

        # Assert
        self.assertEqual("Order(id=O-19700101-000000-001-001-1, state=INITIALIZED, BUY 100K AUD/USD.FXCM MARKET DAY)", str(order))  # noqa
        self.assertTrue(repr(order).startswith("<Order(id=O-19700101-000000-001-001-1, state=INITIALIZED, BUY 100K AUD/USD.FXCM MARKET DAY) object at"))  # noqa

    def test_can_initialize_limit_order(self):
        # Arrange
        # Act
        order = self.order_factory.limit(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000),
            Price(1.00000, 5))

        # Assert
        self.assertEqual(OrderType.LIMIT, order.type)
        self.assertEqual(OrderState.INITIALIZED, order.state)
        self.assertEqual(TimeInForce.DAY, order.time_in_force)
        self.assertFalse(order.is_completed)

    def test_can_initialize_limit_order_with_expire_time(self):
        # Arrange
        # Act
        order = self.order_factory.limit(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000),
            Price(1.00000, 5),
            Label("U1_TP"),
            OrderPurpose.NONE,
            TimeInForce.GTD,
            UNIX_EPOCH)

        # Assert
        self.assertEqual(AUDUSD_FXCM, order.symbol)
        self.assertEqual(OrderType.LIMIT, order.type)
        self.assertEqual(Price(1.00000, 5), order.price)
        self.assertEqual(OrderState.INITIALIZED, order.state)
        self.assertEqual(TimeInForce.GTD, order.time_in_force)
        self.assertEqual(UNIX_EPOCH, order.expire_time)
        self.assertFalse(order.is_completed)

    def test_can_initialize_stop_market_order(self):
        # Arrange
        # Act
        order = self.order_factory.stop(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000),
            Price(1.00000, 5))

        # Assert
        self.assertEqual(OrderType.STOP, order.type)
        self.assertEqual(OrderState.INITIALIZED, order.state)
        self.assertEqual(TimeInForce.DAY, order.time_in_force)
        self.assertFalse(order.is_completed)

    def test_can_initialize_stop_limit_order(self):
        # Arrange
        # Act
        order = self.order_factory.stop_limit(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000),
            Price(1.00000, 5))

        # Assert
        self.assertEqual(OrderType.STOP_LIMIT, order.type)
        self.assertEqual(OrderState.INITIALIZED, order.state)
        self.assertFalse(order.is_completed)

    def test_can_initialize_market_if_touched_order(self):
        # Arrange
        # Act
        order = self.order_factory.market_if_touched(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000),
            Price(1.00000, 5))

        # Assert
        self.assertEqual(OrderType.MIT, order.type)
        self.assertEqual(OrderState.INITIALIZED, order.state)
        self.assertFalse(order.is_completed)

    def test_can_initialize_fill_or_kill_order(self):
        # Arrange
        # Act
        order = self.order_factory.fill_or_kill(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))

        # Assert
        self.assertEqual(OrderType.MARKET, order.type)
        self.assertEqual(TimeInForce.FOC, order.time_in_force)
        self.assertEqual(OrderState.INITIALIZED, order.state)
        self.assertFalse(order.is_completed)

    def test_can_initialize_immediate_or_cancel_order(self):
        # Arrange
        # Act
        order = self.order_factory.immediate_or_cancel(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))

        # Assert
        self.assertEqual(OrderType.MARKET, order.type)
        self.assertEqual(TimeInForce.IOC, order.time_in_force)
        self.assertEqual(OrderState.INITIALIZED, order.state)
        self.assertFalse(order.is_completed)

    def test_can_initialize_bracket_order_market_with_no_take_profit_or_label(self):
        # Arrange
        # Act
        bracket_order = self.order_factory.bracket_market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000),
            Price(0.99990, 5))

        # Assert
        self.assertEqual(AUDUSD_FXCM, bracket_order.stop_loss.symbol)
        self.assertFalse(bracket_order.has_take_profit)
        self.assertEqual(OrderId("O-19700101-000000-001-001-1"), bracket_order.entry.id)
        self.assertEqual(OrderId("O-19700101-000000-001-001-2"), bracket_order.stop_loss.id)
        self.assertEqual(OrderSide.SELL, bracket_order.stop_loss.side)
        self.assertEqual(Quantity(100000), bracket_order.entry.quantity)
        self.assertEqual(Quantity(100000), bracket_order.stop_loss.quantity)
        self.assertEqual(Price(0.99990, 5), bracket_order.stop_loss.price)
        self.assertEqual(None, bracket_order.entry.label)
        self.assertEqual(None, bracket_order.stop_loss.label)
        self.assertEqual(TimeInForce.GTC, bracket_order.stop_loss.time_in_force)
        self.assertEqual(None, bracket_order.entry.expire_time)
        self.assertEqual(None, bracket_order.stop_loss.expire_time)
        self.assertEqual(BracketOrderId("BO-19700101-000000-001-001-1"), bracket_order.id)
        self.assertEqual(UNIX_EPOCH, bracket_order.timestamp)

    def test_can_initialize_bracket_order_market_with_take_profit_and_label(self):
        # Arrange
        # Act
        bracket_order = self.order_factory.bracket_market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000),
            Price(0.99990, 5),
            Price(1.00010, 5),
            Label("U1"))

        # Assert
        self.assertEqual(AUDUSD_FXCM, bracket_order.stop_loss.symbol)
        self.assertTrue(bracket_order.has_take_profit)
        self.assertEqual(AUDUSD_FXCM, bracket_order.take_profit.symbol)
        self.assertEqual(OrderId("O-19700101-000000-001-001-1"), bracket_order.entry.id)
        self.assertEqual(OrderId("O-19700101-000000-001-001-2"), bracket_order.stop_loss.id)
        self.assertEqual(OrderId("O-19700101-000000-001-001-3"), bracket_order.take_profit.id)
        self.assertEqual(OrderSide.SELL, bracket_order.stop_loss.side)
        self.assertEqual(OrderSide.SELL, bracket_order.take_profit.side)
        self.assertEqual(Quantity(100000), bracket_order.stop_loss.quantity)
        self.assertEqual(Quantity(100000), bracket_order.take_profit.quantity)
        self.assertEqual(Price(0.99990, 5), bracket_order.stop_loss.price)
        self.assertEqual(Price(1.00010, 5), bracket_order.take_profit.price)
        self.assertEqual(Label("U1_E"), bracket_order.entry.label)
        self.assertEqual(Label("U1_SL"), bracket_order.stop_loss.label)
        self.assertEqual(Label("U1_TP"), bracket_order.take_profit.label)
        self.assertEqual(TimeInForce.GTC, bracket_order.stop_loss.time_in_force)
        self.assertEqual(TimeInForce.GTC, bracket_order.take_profit.time_in_force)
        self.assertEqual(None, bracket_order.entry.expire_time)
        self.assertEqual(None, bracket_order.stop_loss.expire_time)
        self.assertEqual(None, bracket_order.take_profit.expire_time)
        self.assertEqual(BracketOrderId("BO-19700101-000000-001-001-1"), bracket_order.id)
        self.assertEqual(UNIX_EPOCH, bracket_order.timestamp)

    def test_bracket_order_str_and_repr(self):
        # Arrange
        # Act
        bracket_order = self.order_factory.bracket_market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000),
            Price(0.99990, 5),
            Price(1.00010, 5),
            Label("U1"))

        # Assert
        self.assertEqual("BracketOrder(id=BO-19700101-000000-001-001-1, EntryOrder(id=O-19700101-000000-001-001-1, state=INITIALIZED, label=U1_E, BUY 100K AUD/USD.FXCM MARKET DAY), SL=0.99990, TP=1.00010)", str(bracket_order))  # noqa
        self.assertTrue(repr(bracket_order).startswith("<BracketOrder(id=BO-19700101-000000-001-001-1, EntryOrder(id=O-19700101-000000-001-001-1, state=INITIALIZED, label=U1_E, BUY 100K AUD/USD.FXCM MARKET DAY), SL=0.99990, TP=1.00010) object at"))  # noqa
        self.assertTrue(repr(bracket_order).endswith(">"))

    def test_can_apply_order_submitted_event_to_order(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))

        event = OrderSubmitted(
            self.account_id,
            order.id,
            UNIX_EPOCH,
            uuid4(),
            UNIX_EPOCH)

        # Act
        order.apply(event)

        # Assert
        self.assertEqual(OrderState.SUBMITTED, order.state)
        self.assertEqual(2, order.event_count)
        self.assertEqual(event, order.last_event)
        self.assertFalse(order.is_completed)

    def test_can_apply_order_accepted_event_to_order(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))

        event = OrderAccepted(
            self.account_id,
            order.id,
            OrderIdBroker("B" + order.id.value),
            Label("E"),
            UNIX_EPOCH,
            uuid4(),
            UNIX_EPOCH)

        # Act
        order.apply(event)

        # Assert
        self.assertEqual(OrderState.ACCEPTED, order.state)
        self.assertFalse(order.is_completed)

    def test_can_apply_order_rejected_event_to_order(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))

        event = OrderRejected(
            self.account_id,
            order.id,
            UNIX_EPOCH,
            ValidString("ORDER ID INVALID"),
            uuid4(),
            UNIX_EPOCH)

        # Act
        order.apply(event)

        # Assert
        self.assertEqual(OrderState.REJECTED, order.state)
        self.assertTrue(order.is_completed)

    def test_can_apply_order_working_event_to_order(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))

        event = OrderWorking(
            self.account_id,
            order.id,
            OrderIdBroker("SOME_BROKER_ID"),
            order.symbol,
            order.label,
            order.side,
            order.type,
            order.quantity,
            Price(1.0, 1),
            order.time_in_force,
            UNIX_EPOCH,
            uuid4(),
            UNIX_EPOCH,
            order.expire_time)

        # Act
        order.apply(event)

        # Assert
        # print(order)
        self.assertEqual(OrderState.WORKING, order.state)
        self.assertEqual(OrderIdBroker("SOME_BROKER_ID"), order.id_broker)
        self.assertFalse(order.is_completed)
        self.assertTrue(order.is_working)
        self.assertEqual(None, order.filled_timestamp)

    def test_can_apply_order_expired_event_to_order(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))

        event = OrderExpired(
            self.account_id,
            order.id,
            UNIX_EPOCH,
            uuid4(),
            UNIX_EPOCH)

        # Act
        order.apply(event)

        # Assert
        self.assertEqual(OrderState.EXPIRED, order.state)
        self.assertTrue(order.is_completed)

    def test_can_apply_order_cancelled_event_to_order(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))

        event = OrderCancelled(
            self.account_id,
            order.id,
            UNIX_EPOCH,
            uuid4(),
            UNIX_EPOCH)

        # Act
        order.apply(event)

        # Assert
        self.assertEqual(OrderState.CANCELLED, order.state)
        self.assertTrue(order.is_completed)

    def test_can_apply_order_cancel_reject_event_to_order(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))

        event = OrderCancelReject(
            self.account_id,
            order.id,
            UNIX_EPOCH,
            ValidString("REJECT_RESPONSE"),
            ValidString("ORDER DOES NOT EXIST"),
            uuid4(),
            UNIX_EPOCH)

        # Act
        order.apply(event)

        # Assert
        self.assertEqual(OrderState.INITIALIZED, order.state)

    def test_can_apply_order_modified_event_to_order(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))

        order_working = OrderWorking(
            self.account_id,
            order.id,
            OrderIdBroker("SOME_BROKER_ID_1"),
            order.symbol,
            order.label,
            order.side,
            order.type,
            order.quantity,
            Price(1.00000, 5),
            order.time_in_force,
            UNIX_EPOCH,
            uuid4(),
            UNIX_EPOCH,
            order.expire_time)

        order_modified = OrderModified(
            self.account_id,
            order.id,
            OrderIdBroker("SOME_BROKER_ID_2"),
            Quantity(120000),
            Price(1.00001, 5),
            UNIX_EPOCH,
            uuid4(),
            UNIX_EPOCH)

        order.apply(order_working)

        # Act
        order.apply(order_modified)

        # Assert
        self.assertEqual(OrderState.WORKING, order.state)
        self.assertEqual(OrderIdBroker("SOME_BROKER_ID_2"), order.id_broker)
        self.assertEqual(Quantity(120000), order.quantity)
        self.assertEqual(Price(1.00001, 5), order.price)
        self.assertTrue(order.is_working)
        self.assertFalse(order.is_completed)
        self.assertEqual(3, order.event_count)

    def test_can_apply_order_filled_event_to_market_order(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))

        event = OrderFilled(
            self.account_id,
            order.id,
            ExecutionId("SOME_EXEC_ID_1"),
            PositionIdBroker("SOME_EXEC_TICKET_1"),
            order.symbol,
            order.side,
            order.quantity,
            Price(1.00001, 5),
            Currency.USD,
            UNIX_EPOCH,
            uuid4(),
            UNIX_EPOCH)

        # Act
        order.apply(event)

        # Assert
        self.assertEqual(OrderState.FILLED, order.state)
        self.assertEqual(Quantity(100000), order.filled_quantity)
        self.assertEqual(Price(1.00001, 5), order.average_price)
        self.assertTrue(order.is_completed)
        self.assertEqual(UNIX_EPOCH, order.filled_timestamp)

    def test_can_apply_order_filled_event_to_buy_limit_order(self):
        # Arrange
        order = self.order_factory.limit(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000),
            Price(1.00000, 5))

        event = OrderFilled(
            self.account_id,
            order.id,
            ExecutionId("SOME_EXEC_ID_1"),
            PositionIdBroker("SOME_EXEC_TICKET_1"),
            order.symbol,
            order.side,
            order.quantity,
            Price(1.00001, 5),
            Currency.USD,
            UNIX_EPOCH,
            uuid4(),
            UNIX_EPOCH)

        # Act
        order.apply(event)

        # Assert
        self.assertEqual(OrderState.FILLED, order.state)
        self.assertEqual(Quantity(100000), order.filled_quantity)
        self.assertEqual(Price(1.00000, 5), order.price)
        self.assertEqual(Price(1.00001, 5), order.average_price)
        self.assertEqual(Decimal64(0.00001, 5), order.slippage)
        self.assertTrue(order.is_completed)
        self.assertEqual(UNIX_EPOCH, order.filled_timestamp)

    def test_can_apply_order_partially_filled_event_to_buy_limit_order(self):
        # Arrange
        order = self.order_factory.limit(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000),
            Price(1.00000, 5))

        event = OrderPartiallyFilled(
            self.account_id,
            order.id,
            ExecutionId("SOME_EXEC_ID_1"),
            PositionIdBroker("SOME_EXEC_TICKET_1"),
            order.symbol,
            order.side,
            Quantity(50000),
            Quantity(50000),
            Price(0.999999, 6),
            Currency.USD,
            UNIX_EPOCH,
            uuid4(),
            UNIX_EPOCH)

        # Act
        order.apply(event)

        # Assert
        self.assertEqual(OrderState.PARTIALLY_FILLED, order.state)
        self.assertEqual(Quantity(50000), order.filled_quantity)
        self.assertEqual(Price(1.00000, 5), order.price)
        self.assertEqual(Price(0.999999, 6), order.average_price)
        self.assertEqual(Decimal64(-0.000001, 6), order.slippage)
        self.assertFalse(order.is_completed)
        self.assertEqual(UNIX_EPOCH, order.filled_timestamp)

    def test_can_apply_order_overfilled_event_to_buy_limit_order(self):
        # Arrange
        order = self.order_factory.limit(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000),
            Price(1.00000, 5))

        event = OrderFilled(
            self.account_id,
            order.id,
            ExecutionId("SOME_EXEC_ID_1"),
            PositionIdBroker("SOME_EXEC_TICKET_1"),
            order.symbol,
            order.side,
            Quantity(150000),
            Price(0.99999, 5),
            Currency.USD,
            UNIX_EPOCH,
            uuid4(),
            UNIX_EPOCH)

        # Act
        order.apply(event)

        # Assert
        self.assertEqual(OrderState.OVER_FILLED, order.state)
        self.assertEqual(Quantity(150000), order.filled_quantity)
        self.assertFalse(order.is_completed)
        self.assertEqual(UNIX_EPOCH, order.filled_timestamp)

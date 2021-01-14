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

from decimal import Decimal
import unittest

from parameterized import parameterized

from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.factories import OrderFactory
from nautilus_trader.core.uuid import uuid4
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderState
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.enums import PositionSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.events import OrderDenied
from nautilus_trader.model.events import OrderFilled
from nautilus_trader.model.events import OrderInitialized
from nautilus_trader.model.events import OrderInvalid
from nautilus_trader.model.events import OrderModified
from nautilus_trader.model.identifiers import BracketOrderId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import ExecutionId
from nautilus_trader.model.identifiers import OrderId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.order import MarketOrder
from nautilus_trader.model.order import Order
from nautilus_trader.model.order import StopMarketOrder
from tests.test_kit.providers import TestInstrumentProvider
from tests.test_kit.stubs import TestStubs
from tests.test_kit.stubs import UNIX_EPOCH


AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy(TestStubs.symbol_audusd_fxcm())


class OrderTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        self.account_id = TestStubs.account_id()
        self.order_factory = OrderFactory(
            trader_id=TraderId("TESTER", "000"),
            strategy_id=StrategyId("S", "001"),
            clock=TestClock(),
        )

    def test_opposite_side_given_undefined_raises_value_error(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(ValueError, Order.opposite_side, OrderSide.UNDEFINED)

    def test_flatten_side_given_undefined_or_flat_raises_value_error(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(ValueError, Order.flatten_side, PositionSide.UNDEFINED)
        self.assertRaises(ValueError, Order.flatten_side, PositionSide.FLAT)

    @parameterized.expand([
        [OrderSide.BUY, OrderSide.SELL],
        [OrderSide.SELL, OrderSide.BUY],
    ])
    def test_opposite_side_returns_expected_sides(self, side, expected):
        # Arrange
        # Act
        result = Order.opposite_side(side)

        # Assert
        self.assertEqual(expected, result)

    @parameterized.expand([
        [PositionSide.LONG, OrderSide.SELL],
        [PositionSide.SHORT, OrderSide.BUY],
    ])
    def test_flatten_side_returns_expected_sides(self, side, expected):
        # Arrange
        # Act
        result = Order.flatten_side(side)

        # Assert
        self.assertEqual(expected, result)

    def test_market_order_with_quantity_zero_raises_exception(self):
        # Arrange
        # Act
        self.assertRaises(
            ValueError,
            MarketOrder,
            ClientOrderId("O-123456"),
            StrategyId("S", "001"),
            AUDUSD_SIM.symbol,
            OrderSide.BUY,
            Quantity(),
            TimeInForce.DAY,
            uuid4(),
            UNIX_EPOCH,
        )

    def test_market_order_with_invalid_tif_raises_exception(self):
        # Arrange
        # Act
        self.assertRaises(
            ValueError,
            MarketOrder,
            ClientOrderId("O-123456"),
            StrategyId("S", "001"),
            AUDUSD_SIM.symbol,
            OrderSide.BUY,
            Quantity(100),
            TimeInForce.GTD,
            uuid4(),
            UNIX_EPOCH,
        )

    def test_stop_order_with_gtd_and_expire_time_none_raises_exception(self):
        # Arrange
        # Act
        self.assertRaises(
            TypeError,
            StopMarketOrder,
            ClientOrderId("O-123456"),
            StrategyId("S", "001"),
            AUDUSD_SIM.symbol,
            OrderSide.BUY,
            Quantity(100000),
            price=Price("1.00000"),
            init_id=uuid4(),
            timestamp=UNIX_EPOCH,
            time_in_force=TimeInForce.GTD,
            expire_time=None,
        )

    def test_reset_order_factory(self):
        # Arrange
        self.order_factory.limit(
            AUDUSD_SIM.symbol,
            OrderSide.BUY,
            Quantity(100000),
            Price("1.00000"),
        )

        # Act
        self.order_factory.reset()

        order2 = self.order_factory.limit(
            AUDUSD_SIM.symbol,
            OrderSide.BUY,
            Quantity(100000),
            Price("1.00000"),
        )

        self.assertEqual(ClientOrderId("O-19700101-000000-000-001-1"), order2.cl_ord_id)

    def test_limit_order_can_create_expected_decimal_price(self):
        # Arrange
        # Act
        order1 = self.order_factory.limit(
            AUDUSD_SIM.symbol,
            OrderSide.BUY,
            Quantity(100000),
            Price("1.00000"),
        )

        order2 = self.order_factory.limit(
            AUDUSD_SIM.symbol,
            OrderSide.BUY,
            Quantity(100000),
            Price("1.00000"),
        )

        order3 = self.order_factory.limit(
            AUDUSD_SIM.symbol,
            OrderSide.BUY,
            Quantity(100000),
            Price("1.00000"),
        )

        order4 = self.order_factory.limit(
            AUDUSD_SIM.symbol,
            OrderSide.BUY,
            Quantity(100000),
            Price("1.00001"),
        )

        # Assert
        self.assertEqual(Price("1.00000"), order1.price)
        self.assertEqual(Price("1.00000"), order2.price)
        self.assertEqual(Price("1.00000"), order3.price)
        self.assertEqual(Price("1.00001"), order4.price)

    def test_initialize_buy_market_order(self):
        # Arrange
        # Act
        order = self.order_factory.market(
            AUDUSD_SIM.symbol,
            OrderSide.BUY,
            Quantity(100000),
        )

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
        self.assertEqual(UNIX_EPOCH, order.last_event.timestamp)

    def test_initialize_sell_market_order(self):
        # Arrange
        # Act
        order = self.order_factory.market(
            AUDUSD_SIM.symbol,
            OrderSide.SELL,
            Quantity(100000),
        )

        # Assert
        self.assertEqual(OrderType.MARKET, order.type)
        self.assertEqual(OrderState.INITIALIZED, order.state)
        self.assertEqual(1, order.event_count)
        self.assertTrue(isinstance(order.last_event, OrderInitialized))
        self.assertEqual(1, len(order.events))
        self.assertFalse(order.is_working)
        self.assertFalse(order.is_completed)
        self.assertFalse(order.is_buy)
        self.assertTrue(order.is_sell)
        self.assertEqual(None, order.filled_timestamp)

    def test_order_str_and_repr(self):
        # Arrange
        # Act
        order = self.order_factory.market(
            AUDUSD_SIM.symbol,
            OrderSide.BUY,
            Quantity(100000),
        )

        # Assert
        self.assertEqual("MarketOrder(cl_ord_id=O-19700101-000000-000-001-1, state=INITIALIZED, BUY 100,000 AUD/USD.SIM MARKET DAY)", str(order))  # noqa
        self.assertEqual("MarketOrder(cl_ord_id=O-19700101-000000-000-001-1, state=INITIALIZED, BUY 100,000 AUD/USD.SIM MARKET DAY)", repr(order))  # noqa

    def test_initialize_limit_order(self):
        # Arrange
        # Act
        order = self.order_factory.limit(
            AUDUSD_SIM.symbol,
            OrderSide.BUY,
            Quantity(100000),
            Price("1.00000"),
        )

        # Assert
        self.assertEqual(OrderType.LIMIT, order.type)
        self.assertEqual(OrderState.INITIALIZED, order.state)
        self.assertEqual(TimeInForce.DAY, order.time_in_force)
        self.assertFalse(order.is_completed)

    def test_initialize_limit_order_with_expire_time(self):
        # Arrange
        # Act
        order = self.order_factory.limit(
            AUDUSD_SIM.symbol,
            OrderSide.BUY,
            Quantity(100000),
            Price("1.00000"),
            TimeInForce.GTD,
            UNIX_EPOCH,
        )

        # Assert
        self.assertEqual(AUDUSD_SIM.symbol, order.symbol)
        self.assertEqual(OrderType.LIMIT, order.type)
        self.assertEqual(Price("1.00000"), order.price)
        self.assertEqual(OrderState.INITIALIZED, order.state)
        self.assertEqual(TimeInForce.GTD, order.time_in_force)
        self.assertEqual(UNIX_EPOCH, order.expire_time)
        self.assertFalse(order.is_completed)

    def test_initialize_stop_order(self):
        # Arrange
        # Act
        order = self.order_factory.stop_market(
            AUDUSD_SIM.symbol,
            OrderSide.BUY,
            Quantity(100000),
            Price("1.00000"),
        )

        # Assert
        self.assertEqual(OrderType.STOP_MARKET, order.type)
        self.assertEqual(OrderState.INITIALIZED, order.state)
        self.assertEqual(TimeInForce.DAY, order.time_in_force)
        self.assertFalse(order.is_completed)

    def test_bracket_order_equality(self):
        # Arrange
        entry1 = self.order_factory.market(
            AUDUSD_SIM.symbol,
            OrderSide.BUY,
            Quantity(100000),
        )

        entry2 = self.order_factory.market(
            AUDUSD_SIM.symbol,
            OrderSide.BUY,
            Quantity(100000),
        )

        bracket_order1 = self.order_factory.bracket(entry1, Price("1.00000"))
        bracket_order2 = self.order_factory.bracket(entry2, Price("1.00000"))

        # Act
        # Assert
        self.assertTrue(bracket_order1 == bracket_order1)
        self.assertTrue(bracket_order1 != bracket_order2)

    def test_initialize_bracket_order_market_with_no_take_profit(self):
        # Arrange
        entry_order = self.order_factory.market(
            AUDUSD_SIM.symbol,
            OrderSide.BUY,
            Quantity(100000),
        )

        # Act
        bracket_order = self.order_factory.bracket(entry_order, Price("0.99990"))

        # Assert
        self.assertEqual(AUDUSD_SIM.symbol, bracket_order.stop_loss.symbol)
        self.assertFalse(bracket_order.take_profit is not None)
        self.assertEqual(ClientOrderId("O-19700101-000000-000-001-1"), bracket_order.entry.cl_ord_id)
        self.assertEqual(ClientOrderId("O-19700101-000000-000-001-2"), bracket_order.stop_loss.cl_ord_id)
        self.assertEqual(OrderSide.SELL, bracket_order.stop_loss.side)
        self.assertEqual(Quantity(100000), bracket_order.entry.quantity)
        self.assertEqual(Quantity(100000), bracket_order.stop_loss.quantity)
        self.assertEqual(Price("0.99990"), bracket_order.stop_loss.price)
        self.assertEqual(TimeInForce.GTC, bracket_order.stop_loss.time_in_force)
        self.assertEqual(None, bracket_order.stop_loss.expire_time)
        self.assertEqual(BracketOrderId("BO-19700101-000000-000-001-1"), bracket_order.id)
        self.assertEqual(UNIX_EPOCH, bracket_order.timestamp)

    def test_initialize_bracket_order_stop_with_take_profit(self):
        # Arrange
        entry_order = self.order_factory.stop_market(
            AUDUSD_SIM.symbol,
            OrderSide.BUY,
            Quantity(100000),
            Price("0.99995"),
        )

        # Act
        bracket_order = self.order_factory.bracket(
            entry_order,
            Price("0.99990"),
            Price("1.00010"),
        )

        # Assert
        self.assertEqual(AUDUSD_SIM.symbol, bracket_order.stop_loss.symbol)
        self.assertTrue(bracket_order.take_profit is not None)
        self.assertEqual(AUDUSD_SIM.symbol, bracket_order.take_profit.symbol)
        self.assertEqual(ClientOrderId("O-19700101-000000-000-001-1"), bracket_order.entry.cl_ord_id)
        self.assertEqual(ClientOrderId("O-19700101-000000-000-001-2"), bracket_order.stop_loss.cl_ord_id)
        self.assertEqual(ClientOrderId("O-19700101-000000-000-001-3"), bracket_order.take_profit.cl_ord_id)
        self.assertEqual(OrderSide.SELL, bracket_order.stop_loss.side)
        self.assertEqual(OrderSide.SELL, bracket_order.take_profit.side)
        self.assertEqual(Quantity(100000), bracket_order.stop_loss.quantity)
        self.assertEqual(Quantity(100000), bracket_order.take_profit.quantity)
        self.assertEqual(Price("0.99990"), bracket_order.stop_loss.price)
        self.assertEqual(Price("1.00010"), bracket_order.take_profit.price)
        self.assertEqual(TimeInForce.GTC, bracket_order.stop_loss.time_in_force)
        self.assertEqual(TimeInForce.GTC, bracket_order.take_profit.time_in_force)
        self.assertEqual(None, bracket_order.entry.expire_time)
        self.assertEqual(None, bracket_order.stop_loss.expire_time)
        self.assertEqual(None, bracket_order.take_profit.expire_time)
        self.assertEqual(BracketOrderId("BO-19700101-000000-000-001-1"), bracket_order.id)
        self.assertEqual(UNIX_EPOCH, bracket_order.timestamp)

    def test_bracket_order_str_and_repr(self):
        # Arrange
        # Act
        entry_order = self.order_factory.market(
            AUDUSD_SIM.symbol,
            OrderSide.BUY,
            Quantity(100000),
        )

        bracket_order = self.order_factory.bracket(
            entry_order,
            Price("0.99990"),
            Price("1.00010"),
        )

        # Assert
        self.assertEqual("BracketOrder(id=BO-19700101-000000-000-001-1, EntryMarketOrder(cl_ord_id=O-19700101-000000-000-001-1, state=INITIALIZED, BUY 100,000 AUD/USD.SIM MARKET DAY), SL=0.99990, TP=1.00010)", str(bracket_order))  # noqa
        self.assertEqual("BracketOrder(id=BO-19700101-000000-000-001-1, EntryMarketOrder(cl_ord_id=O-19700101-000000-000-001-1, state=INITIALIZED, BUY 100,000 AUD/USD.SIM MARKET DAY), SL=0.99990, TP=1.00010)", repr(bracket_order))  # noqa

    def test_apply_order_invalid_event(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_SIM.symbol,
            OrderSide.BUY,
            Quantity(100000),
        )

        invalid = OrderInvalid(
            order.cl_ord_id,
            "SOME_REASON",
            uuid4(),
            UNIX_EPOCH,
        )

        # Act
        order.apply(invalid)

        # Assert
        self.assertEqual(OrderState.INVALID, order.state)
        self.assertEqual(2, order.event_count)
        self.assertEqual(invalid, order.last_event)
        self.assertTrue(order.is_completed)

    def test_apply_order_denied_event(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_SIM.symbol,
            OrderSide.BUY,
            Quantity(100000),
        )

        denied = OrderDenied(
            order.cl_ord_id,
            "SOME_REASON",
            uuid4(),
            UNIX_EPOCH,
        )

        # Act
        order.apply(denied)

        # Assert
        self.assertEqual(OrderState.DENIED, order.state)
        self.assertEqual(2, order.event_count)
        self.assertEqual(denied, order.last_event)
        self.assertTrue(order.is_completed)

    def test_apply_order_submitted_event(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_SIM.symbol,
            OrderSide.BUY,
            Quantity(100000),
        )

        submitted = TestStubs.event_order_submitted(order)

        # Act
        order.apply(submitted)

        # Assert
        self.assertEqual(OrderState.SUBMITTED, order.state)
        self.assertEqual(2, order.event_count)
        self.assertEqual(submitted, order.last_event)
        self.assertFalse(order.is_completed)

    def test_apply_order_accepted_event(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_SIM.symbol,
            OrderSide.BUY,
            Quantity(100000),
        )

        order.apply(TestStubs.event_order_submitted(order))

        # Act
        order.apply(TestStubs.event_order_accepted(order))

        # Assert
        self.assertEqual(OrderState.ACCEPTED, order.state)
        self.assertFalse(order.is_completed)

    def test_apply_order_rejected_event(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_SIM.symbol,
            OrderSide.BUY,
            Quantity(100000),
        )

        order.apply(TestStubs.event_order_submitted(order))

        # Act
        order.apply(TestStubs.event_order_rejected(order))

        # Assert
        self.assertEqual(OrderState.REJECTED, order.state)
        self.assertTrue(order.is_completed)

    def test_apply_order_working_event(self):
        # Arrange
        order = self.order_factory.stop_market(
            AUDUSD_SIM.symbol,
            OrderSide.BUY,
            Quantity(100000),
            Price("1.00000"),
        )

        order.apply(TestStubs.event_order_submitted(order))
        order.apply(TestStubs.event_order_accepted(order))

        # Act
        order.apply(TestStubs.event_order_working(order))

        # Assert
        # print(order)
        self.assertEqual(OrderState.WORKING, order.state)
        self.assertEqual(OrderId("1"), order.id)
        self.assertFalse(order.is_completed)
        self.assertTrue(order.is_working)
        self.assertEqual(None, order.filled_timestamp)

    def test_apply_order_expired_event(self):
        # Arrange
        order = self.order_factory.stop_market(
            AUDUSD_SIM.symbol,
            OrderSide.BUY,
            Quantity(100000),
            Price("0.99990"),
            TimeInForce.GTD,
            UNIX_EPOCH,
        )

        order.apply(TestStubs.event_order_submitted(order))
        order.apply(TestStubs.event_order_accepted(order))
        order.apply(TestStubs.event_order_working(order))

        # Act
        order.apply(TestStubs.event_order_expired(order))

        # Assert
        self.assertEqual(OrderState.EXPIRED, order.state)
        self.assertTrue(order.is_completed)

    def test_apply_order_cancelled_event(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_SIM.symbol,
            OrderSide.BUY,
            Quantity(100000),
        )

        order.apply(TestStubs.event_order_submitted(order))
        order.apply(TestStubs.event_order_accepted(order))

        # Act
        order.apply(TestStubs.event_order_cancelled(order))

        # Assert
        self.assertEqual(OrderState.CANCELLED, order.state)
        self.assertTrue(order.is_completed)

    def test_apply_order_modified_event_to_stop_order(self):
        # Arrange
        order = self.order_factory.stop_market(
            AUDUSD_SIM.symbol,
            OrderSide.BUY,
            Quantity(100000),
            Price("1.00000"),
        )

        order.apply(TestStubs.event_order_submitted(order))
        order.apply(TestStubs.event_order_accepted(order))
        order.apply(TestStubs.event_order_working(order))

        modified = OrderModified(
            self.account_id,
            order.cl_ord_id,
            OrderId("1"),
            Quantity(120000),
            Price("1.00001"),
            UNIX_EPOCH,
            uuid4(),
            UNIX_EPOCH,
        )

        # Act
        order.apply(modified)

        # Assert
        self.assertEqual(OrderState.WORKING, order.state)
        self.assertEqual(OrderId("1"), order.id)
        self.assertEqual(Quantity(120000), order.quantity)
        self.assertEqual(Price("1.00001"), order.price)
        self.assertTrue(order.is_working)
        self.assertFalse(order.is_completed)
        self.assertEqual(5, order.event_count)

    def test_apply_order_filled_event_to_market_order(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_SIM.symbol,
            OrderSide.BUY,
            Quantity(100000),
        )

        order.apply(TestStubs.event_order_submitted(order))
        order.apply(TestStubs.event_order_accepted(order))

        filled = TestStubs.event_order_filled(
            order,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S", "001"),
            fill_price=Price("1.00001"),
        )

        # Act
        order.apply(filled)

        # Assert
        self.assertEqual(OrderState.FILLED, order.state)
        self.assertEqual(Quantity(100000), order.filled_qty)
        self.assertEqual(Decimal("1.00001"), order.avg_price)
        self.assertEqual(1, len(order.execution_ids))
        self.assertTrue(order.is_completed)
        self.assertEqual(UNIX_EPOCH, order.filled_timestamp)

    def test_apply_partial_fill_events_to_market_order_results_in_partially_filled(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_SIM.symbol,
            OrderSide.BUY,
            Quantity(100000),
        )

        order.apply(TestStubs.event_order_submitted(order))
        order.apply(TestStubs.event_order_accepted(order))

        fill1 = TestStubs.event_order_filled(
            order,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S", "001"),
            fill_price=Price("1.00001"),
            fill_qty=Quantity(20000),
        )

        fill2 = TestStubs.event_order_filled(
            order,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S", "001"),
            fill_price=Price("1.00002"),
            fill_qty=Quantity(40000),
        )

        # Act
        order.apply(fill1)
        order.apply(fill2)

        # Assert
        self.assertEqual(OrderState.PARTIALLY_FILLED, order.state)
        self.assertEqual(Quantity(60000), order.filled_qty)
        self.assertEqual(Decimal("1.000014"), order.avg_price)
        self.assertEqual(2, len(order.execution_ids))
        self.assertFalse(order.is_completed)
        self.assertEqual(UNIX_EPOCH, order.filled_timestamp)

    def test_apply_partial_fill_events_to_market_order_results_in_filled(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_SIM.symbol,
            OrderSide.BUY,
            Quantity(100000),
        )

        order.apply(TestStubs.event_order_submitted(order))
        order.apply(TestStubs.event_order_accepted(order))

        fill1 = TestStubs.event_order_filled(
            order,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S", "001"),
            fill_price=Price("1.00001"),
            fill_qty=Quantity(20000),
        )

        fill2 = TestStubs.event_order_filled(
            order,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S", "001"),
            fill_price=Price("1.00002"),
            fill_qty=Quantity(40000),
        )

        fill3 = TestStubs.event_order_filled(
            order,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S", "001"),
            fill_price=Price("1.00003"),
            fill_qty=Quantity(40000),
        )

        # Act
        order.apply(fill1)
        order.apply(fill2)
        order.apply(fill3)

        # Assert
        self.assertEqual(OrderState.FILLED, order.state)
        self.assertEqual(Quantity(100000), order.filled_qty)
        self.assertEqual(Decimal("1.000018571428571428571428571"), order.avg_price)
        self.assertEqual(3, len(order.execution_ids))
        self.assertTrue(order.is_completed)
        self.assertEqual(UNIX_EPOCH, order.filled_timestamp)

    def test_apply_order_filled_event_to_buy_limit_order(self):
        # Arrange
        order = self.order_factory.limit(
            AUDUSD_SIM.symbol,
            OrderSide.BUY,
            Quantity(100000),
            Price("1.00000"),
        )

        order.apply(TestStubs.event_order_submitted(order))
        order.apply(TestStubs.event_order_accepted(order))
        order.apply(TestStubs.event_order_working(order))

        filled = OrderFilled(
            self.account_id,
            order.cl_ord_id,
            OrderId("1"),
            ExecutionId("E-1"),
            PositionId("P-1"),
            StrategyId.null(),
            order.symbol,
            order.side,
            order.quantity,
            order.quantity,
            Quantity(),
            Price("1.00001"),
            AUDUSD_SIM.quote_currency,
            AUDUSD_SIM.is_inverse,
            Money(0, USD),
            LiquiditySide.MAKER,
            UNIX_EPOCH,
            uuid4(),
            UNIX_EPOCH,
        )

        # Act
        order.apply(filled)

        # Assert
        self.assertEqual(OrderState.FILLED, order.state)
        self.assertEqual(Quantity(100000), order.filled_qty)
        self.assertEqual(Price("1.00000"), order.price)
        self.assertEqual(Decimal("1.00001"), order.avg_price)
        self.assertEqual(Decimal("0.00001"), order.slippage)
        self.assertTrue(order.is_completed)
        self.assertEqual(UNIX_EPOCH, order.filled_timestamp)

    def test_apply_order_partially_filled_event_to_buy_limit_order(self):
        # Arrange
        order = self.order_factory.limit(
            AUDUSD_SIM.symbol,
            OrderSide.BUY,
            Quantity(100000),
            Price("1.00000"),
        )

        order.apply(TestStubs.event_order_submitted(order))
        order.apply(TestStubs.event_order_accepted(order))
        order.apply(TestStubs.event_order_working(order))

        partially = OrderFilled(
            self.account_id,
            order.cl_ord_id,
            OrderId("1"),
            ExecutionId("E-1"),
            PositionId("P-1"),
            StrategyId.null(),
            order.symbol,
            order.side,
            Quantity(50000),
            Quantity(50000),
            Quantity(50000),
            Price("0.999999"),
            AUDUSD_SIM.quote_currency,
            AUDUSD_SIM.is_inverse,
            Money(0, USD),
            LiquiditySide.MAKER,
            UNIX_EPOCH,
            uuid4(),
            UNIX_EPOCH,
        )

        # Act
        order.apply(partially)

        # Assert
        self.assertEqual(OrderState.PARTIALLY_FILLED, order.state)
        self.assertEqual(Quantity(50000), order.filled_qty)
        self.assertEqual(Price("1.00000"), order.price)
        self.assertEqual(Decimal("0.999999"), order.avg_price)
        self.assertEqual(Decimal("-0.000001"), order.slippage)
        self.assertFalse(order.is_completed)
        self.assertEqual(UNIX_EPOCH, order.filled_timestamp)

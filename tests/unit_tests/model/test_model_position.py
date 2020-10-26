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

from datetime import datetime
from datetime import timedelta
import unittest

from parameterized import parameterized
import pytz

from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.factories import OrderFactory
from nautilus_trader.core.decimal import Decimal
from nautilus_trader.core.uuid import uuid4
from nautilus_trader.model.currencies import AUD
from nautilus_trader.model.currencies import BTC
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.currencies import USDT
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import PositionSide
from nautilus_trader.model.events import OrderFilled
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import ExecutionId
from nautilus_trader.model.identifiers import OrderId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.position import Position
from nautilus_trader.model.tick import QuoteTick
from tests.test_kit.stubs import TestStubs
from tests.test_kit.stubs import UNIX_EPOCH

AUDUSD_FXCM = TestStubs.symbol_audusd_fxcm()
GBPUSD_FXCM = TestStubs.symbol_gbpusd_fxcm()
BTCUSD_BINANCE = TestStubs.symbol_btcusdt_binance()


class PositionTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        self.account_id = TestStubs.account_id()
        self.order_factory = OrderFactory(
            trader_id=TraderId("TESTER", "000"),
            strategy_id=StrategyId("S", "001"),
            clock=TestClock(),
        )
        print("\n")

    def test_side_from_order_side_given_undefined_raises_value_error(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(ValueError, Position.side_from_order_side, OrderSide.UNDEFINED)

    @parameterized.expand([
        [OrderSide.BUY, PositionSide.LONG],
        [OrderSide.SELL, PositionSide.SHORT],
    ])
    def test_side_from_order_side_given_valid_sides_returns_expected_side(self, order_side, expected):
        # Arrange
        # Act
        position_side = Position.side_from_order_side(order_side)

        # Assert
        self.assertEqual(expected, position_side)

    def test_position_filled_with_buy_order_returns_expected_attributes(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000),
        )

        fill = TestStubs.event_order_filled(
            order,
            PositionId("P-123456"),
            StrategyId("S", "001"),
            Price("1.00001"),
        )

        last = QuoteTick(
            AUDUSD_FXCM,
            Price("1.00050"),
            Price("1.00048"),
            Quantity(1),
            Quantity(1),
            UNIX_EPOCH,
        )

        # Act
        position = Position(fill)

        # Assert
        self.assertEqual(ClientOrderId("O-19700101-000000-000-001-1"), position.from_order)
        self.assertEqual(Quantity(100000), position.quantity)
        self.assertEqual(Quantity(100000), position.peak_quantity)
        self.assertEqual(OrderSide.BUY, position.entry)
        self.assertEqual(PositionSide.LONG, position.side)
        self.assertEqual(UNIX_EPOCH, position.opened_time)
        self.assertIsNone(position.open_duration)
        self.assertEqual(Decimal("1.00001"), position.avg_open)
        self.assertEqual(1, position.event_count)
        self.assertEqual({order.cl_ord_id}, position.cl_ord_ids)
        self.assertEqual({ExecutionId("E-19700101-000000-000-001-1")}, position.execution_ids)
        self.assertEqual(ExecutionId("E-19700101-000000-000-001-1"), position.last_execution_id)
        self.assertEqual(PositionId("P-123456"), position.id)
        self.assertTrue(position.is_long)
        self.assertFalse(position.is_short)
        self.assertFalse(position.is_closed)
        self.assertEqual(0, position.realized_points)
        self.assertEqual(0, position.realized_return)
        self.assertEqual(Money(0, USD), position.realized_pnl)
        self.assertEqual(Money(49.00, USD), position.unrealized_pnl(last))
        self.assertEqual(Money(49.00, USD), position.total_pnl(last))

    def test_position_filled_with_sell_order_returns_expected_attributes(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.SELL,
            Quantity(100000),
        )

        fill = TestStubs.event_order_filled(
            order,
            PositionId("P-123456"),
            StrategyId("S", "001"),
            Price("1.00001"),
        )

        last = QuoteTick(
            AUDUSD_FXCM,
            Price("1.00050"),
            Price("1.00048"),
            Quantity(1),
            Quantity(1),
            UNIX_EPOCH,
        )

        # Act
        position = Position(fill)

        # Assert
        self.assertEqual(Quantity(100000), position.quantity)
        self.assertEqual(Quantity(100000), position.peak_quantity)
        self.assertEqual(PositionSide.SHORT, position.side)
        self.assertEqual(UNIX_EPOCH, position.opened_time)
        self.assertEqual(Decimal("1.00001"), position.avg_open)
        self.assertEqual(1, position.event_count)
        self.assertEqual({ExecutionId("E-19700101-000000-000-001-1")}, position.execution_ids)
        self.assertEqual(ExecutionId("E-19700101-000000-000-001-1"), position.last_execution_id)
        self.assertEqual(PositionId("P-123456"), position.id)
        self.assertFalse(position.is_long)
        self.assertTrue(position.is_short)
        self.assertFalse(position.is_closed)
        self.assertEqual(0, position.realized_points)
        self.assertEqual(0, position.realized_return)
        self.assertEqual(Money(0, USD), position.realized_pnl)
        self.assertEqual(Money(-47.00, USD), position.unrealized_pnl(last))
        self.assertEqual(Money(-47.00, USD), position.total_pnl(last))

    def test_position_partial_fills_with_buy_order_returns_expected_attributes(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000),
        )

        fill = TestStubs.event_order_filled(
            order,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S", "001"),
            fill_price=Price("1.00001"),
            filled_qty=Quantity(50000),
            leaves_qty=Quantity(50000),
        )

        last = QuoteTick(
            AUDUSD_FXCM,
            Price("1.00050"),
            Price("1.00048"),
            Quantity(1),
            Quantity(1),
            UNIX_EPOCH,
        )

        position = Position(fill)

        # Act
        # Assert
        self.assertEqual(Quantity(50000), position.quantity)
        self.assertEqual(Quantity(50000), position.peak_quantity)
        self.assertEqual(PositionSide.LONG, position.side)
        self.assertEqual(UNIX_EPOCH, position.opened_time)
        self.assertEqual(Decimal("1.00001"), position.avg_open)
        self.assertEqual(1, position.event_count)
        self.assertTrue(position.is_long)
        self.assertFalse(position.is_short)
        self.assertFalse(position.is_closed)
        self.assertEqual(0, position.realized_points)
        self.assertEqual(0, position.realized_return)
        self.assertEqual(Money(0, USD), position.realized_pnl)
        self.assertEqual(Money(24.50, USD), position.unrealized_pnl(last))
        self.assertEqual(Money(24.50, USD), position.total_pnl(last))

    def test_position_partial_fills_with_sell_order_returns_expected_attributes(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.SELL,
            Quantity(100000),
        )

        fill1 = TestStubs.event_order_filled(
            order,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S", "001"),
            fill_price=Price("1.00001"),
            filled_qty=Quantity(50000),
            leaves_qty=Quantity(50000),
        )

        fill2 = TestStubs.event_order_filled(
            order,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S", "001"),
            fill_price=Price("1.00002"),
            filled_qty=Quantity(50000),
            leaves_qty=Quantity(),
        )

        position = Position(fill1)

        last = QuoteTick(
            AUDUSD_FXCM,
            Price("1.00050"),
            Price("1.00048"),
            Quantity(1),
            Quantity(1),
            UNIX_EPOCH)

        # Act
        position.apply(fill2)

        # Assert
        self.assertEqual(Quantity(100000), position.quantity)
        self.assertEqual(PositionSide.SHORT, position.side)
        self.assertEqual(UNIX_EPOCH, position.opened_time)
        self.assertEqual(Decimal("1.000015"), position.avg_open)
        self.assertEqual(2, position.event_count)
        self.assertFalse(position.is_long)
        self.assertTrue(position.is_short)
        self.assertFalse(position.is_closed)
        self.assertEqual(0, position.realized_points)
        self.assertEqual(0, position.realized_return)
        self.assertEqual(Money(0, USD), position.realized_pnl)
        self.assertEqual(Money(-46.50, USD), position.unrealized_pnl(last))
        self.assertEqual(Money(-46.50, USD), position.total_pnl(last))

    def test_position_filled_with_buy_order_then_sell_order_returns_expected_attributes(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000),
        )

        fill1 = TestStubs.event_order_filled(
            order,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S", "001"),
            fill_price=Price("1.00001"),
        )

        position = Position(fill1)

        fill2 = OrderFilled(
            self.account_id,
            order.cl_ord_id,
            OrderId("2"),
            ExecutionId("E2"),
            PositionId("T123456"),
            StrategyId("S", "001"),
            order.symbol,
            OrderSide.SELL,
            order.quantity,
            order.quantity,
            Quantity(),
            Price("1.00001"),
            Money(0, USD),
            LiquiditySide.TAKER,
            AUD,
            USD,
            False,
            UNIX_EPOCH + timedelta(minutes=1),
            uuid4(),
            UNIX_EPOCH,
        )

        last = QuoteTick(
            AUDUSD_FXCM,
            Price("1.00050"),
            Price("1.00048"),
            Quantity(1),
            Quantity(1),
            UNIX_EPOCH,
        )

        # Act
        position.apply(fill2)

        # Assert
        self.assertEqual(Quantity(), position.quantity)
        self.assertEqual(PositionSide.FLAT, position.side)
        self.assertEqual(UNIX_EPOCH, position.opened_time)
        self.assertEqual(timedelta(minutes=1), position.open_duration)
        self.assertEqual(Decimal("1.00001"), position.avg_open)
        self.assertEqual(2, position.event_count)
        self.assertEqual(datetime(1970, 1, 1, 0, 1, tzinfo=pytz.utc), position.closed_time)
        self.assertEqual(Decimal("1.00001"), position.avg_close)
        self.assertFalse(position.is_long)
        self.assertFalse(position.is_short)
        self.assertTrue(position.is_closed)
        self.assertEqual(0, position.realized_points)
        self.assertEqual(0, position.realized_return)
        self.assertEqual(Money(0, USD), position.realized_pnl)
        self.assertEqual(Money(0, USD), position.unrealized_pnl(last))
        self.assertEqual(Money(0, USD), position.total_pnl(last))

    def test_position_filled_with_sell_order_then_buy_order_returns_expected_attributes(self):
        # Arrange
        order1 = self.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.SELL,
            Quantity(100000),
        )

        order2 = self.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000),
        )

        fill1 = TestStubs.event_order_filled(order1)

        position = Position(fill1)

        fill2 = TestStubs.event_order_filled(
            order2,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S", "001"),
            fill_price=Price("1.00001"),
            filled_qty=Quantity(50000),
            leaves_qty=Quantity(50000),
        )

        fill3 = TestStubs.event_order_filled(
            order2,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S", "001"),
            fill_price=Price("1.00003"),
            filled_qty=Quantity(50000),
            leaves_qty=Quantity(0),
        )

        last = QuoteTick(
            AUDUSD_FXCM,
            Price("1.00050"),
            Price("1.00048"),
            Quantity(1),
            Quantity(1),
            UNIX_EPOCH,
        )

        # Act
        position.apply(fill2)
        position.apply(fill3)

        # Assert
        self.assertEqual(Quantity(), position.quantity)
        self.assertEqual(PositionSide.FLAT, position.side)
        self.assertEqual(UNIX_EPOCH, position.opened_time)
        self.assertEqual(Decimal("1.0"), position.avg_open)
        self.assertEqual(3, position.event_count)
        self.assertEqual({order1.cl_ord_id, order2.cl_ord_id}, position.cl_ord_ids)
        self.assertEqual(UNIX_EPOCH, position.closed_time)
        self.assertEqual(Decimal("1.00002"), position.avg_close)
        self.assertFalse(position.is_long)
        self.assertFalse(position.is_short)
        self.assertTrue(position.is_closed)
        self.assertEqual(Money(-2.000, USD), position.realized_pnl)
        self.assertEqual(Money(0, USD), position.unrealized_pnl(last))
        self.assertEqual(Money(-2.000, USD), position.total_pnl(last))

    def test_position_filled_with_no_change_returns_expected_attributes(self):
        # Arrange
        order1 = self.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))

        order2 = self.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.SELL,
            Quantity(100000))

        fill1 = TestStubs.event_order_filled(order1)

        position = Position(fill1)

        fill2 = TestStubs.event_order_filled(
            order2,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S", "001"),
            fill_price=Price("1.00000"),
        )

        last = QuoteTick(
            AUDUSD_FXCM,
            Price("1.00050"),
            Price("1.00048"),
            Quantity(1),
            Quantity(1),
            UNIX_EPOCH,
        )

        # Act
        position.apply(fill2)

        # Assert
        self.assertEqual(Quantity(), position.quantity)
        self.assertEqual(PositionSide.FLAT, position.side)
        self.assertEqual(UNIX_EPOCH, position.opened_time)
        self.assertEqual(Decimal("1.0"), position.avg_open)
        self.assertEqual(2, position.event_count)
        self.assertEqual({order1.cl_ord_id, order2.cl_ord_id}, position.cl_ord_ids)
        self.assertEqual({
            ExecutionId("E-19700101-000000-000-001-1"),
            ExecutionId("E-19700101-000000-000-001-2")
        },
            position.execution_ids,
        )
        self.assertEqual(UNIX_EPOCH, position.closed_time)
        self.assertEqual(Decimal("1.0"), position.avg_close)
        self.assertFalse(position.is_long)
        self.assertFalse(position.is_short)
        self.assertTrue(position.is_closed)
        self.assertEqual(0, position.realized_points)
        self.assertEqual(0, position.realized_return)
        self.assertEqual(Money(0, USD), position.realized_pnl)
        self.assertEqual(Money(0, USD), position.unrealized_pnl(last))
        self.assertEqual(Money(0, USD), position.total_pnl(last))

    def test_position_long_with_multiple_filled_orders_returns_expected_attributes(self):
        # Arrange
        order1 = self.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000),
        )

        order2 = self.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000),
        )

        order3 = self.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.SELL,
            Quantity(200000),
        )

        fill1 = TestStubs.event_order_filled(order1, PositionId("P-123456"), StrategyId("S", "001"))
        fill2 = TestStubs.event_order_filled(order2, PositionId("P-123456"), StrategyId("S", "001"), fill_price=Price("1.00001"))
        fill3 = TestStubs.event_order_filled(order3, PositionId("P-123456"), StrategyId("S", "001"), fill_price=Price("1.00010"))

        last = QuoteTick(
            AUDUSD_FXCM,
            Price("1.00050"),
            Price("1.00048"),
            Quantity(1),
            Quantity(1),
            UNIX_EPOCH,
        )

        # Act
        position = Position(fill1)
        position.apply(fill2)
        position.apply(fill3)

        # Assert
        self.assertEqual(Quantity(), position.quantity)
        self.assertEqual(PositionSide.FLAT, position.side)
        self.assertEqual(UNIX_EPOCH, position.opened_time)
        self.assertEqual(Decimal("1.000005"), position.avg_open)
        self.assertEqual(3, position.event_count)
        self.assertEqual({order1.cl_ord_id, order2.cl_ord_id, order3.cl_ord_id}, position.cl_ord_ids)
        self.assertEqual(UNIX_EPOCH, position.closed_time)
        self.assertEqual(Decimal("1.0001"), position.avg_close)
        self.assertFalse(position.is_long)
        self.assertFalse(position.is_short)
        self.assertTrue(position.is_closed)
        self.assertEqual(Money(19.00, USD), position.realized_pnl)
        self.assertEqual(Money(0, USD), position.unrealized_pnl(last))
        self.assertEqual(Money(19.00, USD), position.total_pnl(last))

    def test_position_realised_pnl_with_interleaved_orders_sides(self):
        # Arrange
        order1 = self.order_factory.market(
            BTCUSD_BINANCE,
            OrderSide.BUY,
            Quantity(12),
        )

        order2 = self.order_factory.market(
            BTCUSD_BINANCE,
            OrderSide.BUY,
            Quantity(17),
        )

        order3 = self.order_factory.market(
            BTCUSD_BINANCE,
            OrderSide.SELL,
            Quantity(9),
        )

        order4 = self.order_factory.market(
            BTCUSD_BINANCE,
            OrderSide.BUY,
            Quantity(3),
        )

        order5 = self.order_factory.market(
            BTCUSD_BINANCE,
            OrderSide.SELL,
            Quantity(4),
        )

        # Act
        fill1 = TestStubs.event_order_filled(
            order1,
            fill_price=Price("10000.00"),
            base_currency=BTC,
            quote_currency=USDT,
        )
        position = Position(fill1)

        fill2 = TestStubs.event_order_filled(
            order2,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S", "001"),
            fill_price=Price("9999.00"),
            base_currency=BTC,
            quote_currency=USDT,
        )
        position.apply(fill2)
        self.assertEqual(Quantity(29), position.quantity)
        self.assertEqual(Money(0, BTC), position.realized_pnl)
        self.assertEqual(9999.413793103447, position.avg_open.as_double())

        fill3 = TestStubs.event_order_filled(
            order3,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S", "001"),
            fill_price=Price("10001.00"),
            base_currency=BTC,
            quote_currency=USDT,
        )

        position.apply(fill3)
        self.assertEqual(Quantity(20), position.quantity)
        self.assertEqual(Money(0.00142767, BTC), position.realized_pnl)
        self.assertEqual(9999.413793103447, position.avg_open.as_double())

        fill4 = TestStubs.event_order_filled(
            order4,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S", "001"),
            fill_price=Price("10003.00"),
            base_currency=BTC,
            quote_currency=USDT,
        )
        position.apply(fill4)
        self.assertEqual(Quantity(23), position.quantity)
        self.assertEqual(Money(0.00142767, BTC), position.realized_pnl)
        self.assertEqual(9999.88155922039, position.avg_open.as_double())

        fill5 = TestStubs.event_order_filled(
            order5,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S", "001"),
            fill_price=Price("10005"),
            base_currency=BTC,
            quote_currency=USDT,
        )

        position.apply(fill5)
        self.assertEqual(Quantity(19), position.quantity)
        self.assertEqual(Money(0.00347507, BTC), position.realized_pnl)
        self.assertEqual(9999.88155922039, position.avg_open.as_double())

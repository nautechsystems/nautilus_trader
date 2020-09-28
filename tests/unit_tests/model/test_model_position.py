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

import pytz

from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.factories import OrderFactory
from nautilus_trader.core.uuid import uuid4
from nautilus_trader.model.enums import Currency
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import PositionSide
from nautilus_trader.model.events import OrderFilled
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import ExecutionId
from nautilus_trader.model.identifiers import IdTag
from nautilus_trader.model.identifiers import OrderId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.position import Position
from nautilus_trader.model.tick import QuoteTick
from tests.test_kit.stubs import TestStubs
from tests.test_kit.stubs import UNIX_EPOCH

AUDUSD_FXCM = TestStubs.symbol_audusd_fxcm()
GBPUSD_FXCM = TestStubs.symbol_gbpusd_fxcm()


class PositionTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        self.account_id = TestStubs.account_id()
        self.order_factory = OrderFactory(
            id_tag_trader=IdTag("001"),
            id_tag_strategy=IdTag("001"),
            clock=TestClock())
        print("\n")

    def test_position_filled_with_buy_order_returns_expected_attributes(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))

        fill = TestStubs.event_order_filled(order, PositionId("P-123456"), Price(1.00001, 5))

        last = QuoteTick(
            AUDUSD_FXCM,
            Price(1.00050, 5),
            Price(1.00048, 5),
            Quantity(1),
            Quantity(1),
            UNIX_EPOCH)

        # Act
        position = Position(fill)

        # Assert
        self.assertEqual(ClientOrderId("O-19700101-000000-001-001-1"), position.from_order)
        self.assertEqual(Quantity(100000), position.quantity)
        self.assertEqual(Quantity(100000), position.peak_quantity)
        self.assertEqual(OrderSide.BUY, position.entry)
        self.assertEqual(PositionSide.LONG, position.side)
        self.assertEqual(UNIX_EPOCH, position.opened_time)
        self.assertIsNone(position.open_duration)
        self.assertEqual(1.00001, position.avg_open_price)
        self.assertEqual(1, position.event_count())
        self.assertEqual([order.cl_ord_id], position.get_order_ids())
        self.assertEqual([ExecutionId("E-19700101-000000-001-001-1")], position.get_execution_ids())
        self.assertEqual(ExecutionId("E-19700101-000000-001-001-1"), position.last_execution_id())
        self.assertEqual(PositionId("P-123456"), position.id)
        self.assertTrue(position.is_long())
        self.assertFalse(position.is_short())
        self.assertFalse(position.is_closed())
        self.assertEqual(0.0, position.realized_points)
        self.assertEqual(0.0, position.realized_return)
        self.assertEqual(Money(0, Currency.USD), position.realized_pnl)
        self.assertEqual(0.0004899999999998794, position.unrealized_points(last))
        self.assertEqual(0.0004899951000488789, position.unrealized_return(last))
        self.assertEqual(Money(49.00, Currency.USD), position.unrealized_pnl(last))
        self.assertEqual(0.0004899999999998794, position.total_points(last))
        self.assertEqual(0.0004899951000488789, position.total_return(last))
        self.assertEqual(Money(49.00, Currency.USD), position.total_pnl(last))

    def test_position_filled_with_sell_order_returns_expected_attributes(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.SELL,
            Quantity(100000))

        fill = TestStubs.event_order_filled(order, PositionId("P-123456"), Price(1.00001, 5))

        last = QuoteTick(
            AUDUSD_FXCM,
            Price(1.00050, 5),
            Price(1.00048, 5),
            Quantity(1),
            Quantity(1),
            UNIX_EPOCH)

        # Act
        position = Position(fill)

        # Assert
        self.assertEqual(Quantity(100000), position.quantity)
        self.assertEqual(Quantity(100000), position.peak_quantity)
        self.assertEqual(PositionSide.SHORT, position.side)
        self.assertEqual(UNIX_EPOCH, position.opened_time)
        self.assertEqual(1.00001, position.avg_open_price)
        self.assertEqual(1, position.event_count())
        self.assertEqual([ExecutionId("E-19700101-000000-001-001-1")], position.get_execution_ids())
        self.assertEqual(ExecutionId("E-19700101-000000-001-001-1"), position.last_execution_id())
        self.assertEqual(PositionId("P-123456"), position.id)
        self.assertFalse(position.is_long())
        self.assertTrue(position.is_short())
        self.assertFalse(position.is_closed())
        self.assertEqual(0.0, position.realized_points)
        self.assertEqual(0.0, position.realized_return)
        self.assertEqual(Money(0, Currency.USD), position.realized_pnl)
        self.assertEqual(-0.00046999999999997044, position.unrealized_points(last))
        self.assertEqual(-0.0004699953000469699, position.unrealized_return(last))
        self.assertEqual(Money(-47.00, Currency.USD), position.unrealized_pnl(last))
        self.assertEqual(-0.00046999999999997044, position.total_points(last))
        self.assertEqual(-0.0004699953000469699, position.total_return(last))
        self.assertEqual(Money(-47.00, Currency.USD), position.total_pnl(last))

    def test_position_partial_fills_with_buy_order_returns_expected_attributes(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))

        fill = TestStubs.event_order_filled(
            order,
            position_id=PositionId("P-123456"),
            fill_price=Price(1.00001, 5),
            filled_qty=Quantity(50000),
            leaves_qty=Quantity(50000),
        )

        last = QuoteTick(
            AUDUSD_FXCM,
            Price(1.00050, 5),
            Price(1.00048, 5),
            Quantity(1),
            Quantity(1),
            UNIX_EPOCH)

        position = Position(fill)

        # Act
        # Assert
        self.assertEqual(Quantity(50000), position.quantity)
        self.assertEqual(Quantity(50000), position.peak_quantity)
        self.assertEqual(PositionSide.LONG, position.side)
        self.assertEqual(UNIX_EPOCH, position.opened_time)
        self.assertEqual(1.00001, position.avg_open_price)
        self.assertEqual(1, position.event_count())
        self.assertTrue(position.is_long())
        self.assertFalse(position.is_short())
        self.assertFalse(position.is_closed())
        self.assertEqual(0.0, position.realized_points)
        self.assertEqual(0.0, position.realized_return)
        self.assertEqual(Money(0, Currency.USD), position.realized_pnl)
        self.assertEqual(0.0004899999999998794, position.unrealized_points(last))
        self.assertEqual(0.0004899951000488789, position.unrealized_return(last))
        self.assertEqual(Money(24.50, Currency.USD), position.unrealized_pnl(last))
        self.assertEqual(0.0004899999999998794, position.total_points(last))
        self.assertEqual(0.0004899951000488789, position.total_return(last))
        self.assertEqual(Money(24.50, Currency.USD), position.total_pnl(last))

    def test_position_partial_fills_with_sell_order_returns_expected_attributes(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.SELL,
            Quantity(100000))

        fill1 = TestStubs.event_order_filled(
            order,
            position_id=PositionId("P-123456"),
            fill_price=Price(1.00001, 5),
            filled_qty=Quantity(50000),
            leaves_qty=Quantity(50000),
        )

        fill2 = TestStubs.event_order_filled(
            order,
            position_id=PositionId("P-123456"),
            fill_price=Price(1.00002, 5),
            filled_qty=Quantity(100000),
            leaves_qty=Quantity(0),
        )

        position = Position(fill1)

        last = QuoteTick(
            AUDUSD_FXCM,
            Price(1.00050, 5),
            Price(1.00048, 5),
            Quantity(1),
            Quantity(1),
            UNIX_EPOCH)

        # Act
        position.apply(fill2)

        # Assert
        self.assertEqual(Quantity(100000), position.quantity)
        self.assertEqual(PositionSide.SHORT, position.side)
        self.assertEqual(UNIX_EPOCH, position.opened_time)
        self.assertEqual(1.00002, position.avg_open_price)
        self.assertEqual(2, position.event_count())
        self.assertFalse(position.is_long())
        self.assertTrue(position.is_short())
        self.assertFalse(position.is_closed())
        self.assertEqual(0.0, position.realized_points)
        self.assertEqual(0.0, position.realized_return)
        self.assertEqual(Money(0, Currency.USD), position.realized_pnl)
        self.assertEqual(-0.000460000000000127, position.unrealized_points(last))
        self.assertEqual(-0.00045999080018412335, position.unrealized_return(last))
        self.assertEqual(Money(-46.00, Currency.USD), position.unrealized_pnl(last))
        self.assertEqual(-0.000460000000000127, position.total_points(last))
        self.assertEqual(-0.00045999080018412335, position.total_return(last))
        self.assertEqual(Money(-46.00, Currency.USD), position.total_pnl(last))

    def test_position_filled_with_buy_order_then_sell_order_returns_expected_attributes(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))

        fill1 = TestStubs.event_order_filled(
            order,
            position_id=PositionId("P-123456"),
            fill_price=Price(1.00001, 5),
        )

        position = Position(fill1)

        fill2 = OrderFilled(
            self.account_id,
            order.cl_ord_id,
            OrderId('2'),
            ExecutionId("E2"),
            PositionId("T123456"),
            order.symbol,
            OrderSide.SELL,
            order.quantity,
            Quantity(0),
            Price(1.00001, 5),
            Money(0., Currency.USD),
            LiquiditySide.TAKER,
            Currency.AUD,
            Currency.USD,
            UNIX_EPOCH + timedelta(minutes=1),
            uuid4(),
            UNIX_EPOCH)

        last = QuoteTick(
            AUDUSD_FXCM,
            Price(1.00050, 5),
            Price(1.00048, 5),
            Quantity(1),
            Quantity(1),
            UNIX_EPOCH)

        # Act
        position.apply(fill2)

        # Assert
        self.assertEqual(Quantity(), position.quantity)
        self.assertEqual(PositionSide.FLAT, position.side)
        self.assertEqual(UNIX_EPOCH, position.opened_time)
        self.assertEqual(timedelta(minutes=1), position.open_duration)
        self.assertEqual(1.00001, position.avg_open_price)
        self.assertEqual(2, position.event_count())
        self.assertEqual(datetime(1970, 1, 1, 0, 1, tzinfo=pytz.utc), position.closed_time)
        self.assertEqual(1.00001, position.avg_close_price)
        self.assertFalse(position.is_long())
        self.assertFalse(position.is_short())
        self.assertTrue(position.is_closed())
        self.assertEqual(0.0, position.realized_points)
        self.assertEqual(0.0, position.realized_return)
        self.assertEqual(Money(0, Currency.USD), position.realized_pnl)
        self.assertEqual(0.0, position.unrealized_points(last))
        self.assertEqual(0.0, position.unrealized_return(last))
        self.assertEqual(Money(0, Currency.USD), position.unrealized_pnl(last))
        self.assertEqual(0.0, position.total_points(last))
        self.assertEqual(0.0, position.total_return(last))
        self.assertEqual(Money(0, Currency.USD), position.total_pnl(last))

    def test_position_filled_with_sell_order_then_buy_order_returns_expected_attributes(self):
        # Arrange
        order1 = self.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.SELL,
            Quantity(100000))

        order2 = self.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))

        fill1 = TestStubs.event_order_filled(order1)

        position = Position(fill1)

        fill2 = TestStubs.event_order_filled(
            order2,
            position_id=PositionId("P-123456"),
            fill_price=Price(1.00001, 5),
            filled_qty=Quantity(50000),
            leaves_qty=Quantity(50000),
        )

        fill3 = TestStubs.event_order_filled(
            order2,
            position_id=PositionId("P-123456"),
            fill_price=Price(1.00003, 5),
            filled_qty=Quantity(100000),
            leaves_qty=Quantity(50000),
        )

        last = QuoteTick(
            AUDUSD_FXCM,
            Price(1.00050, 5),
            Price(1.00048, 5),
            Quantity(1),
            Quantity(1),
            UNIX_EPOCH)

        # Act
        position.apply(fill2)
        position.apply(fill3)

        # Assert
        self.assertEqual(Quantity(), position.quantity)
        self.assertEqual(PositionSide.FLAT, position.side)
        self.assertEqual(UNIX_EPOCH, position.opened_time)
        self.assertEqual(1.0, position.avg_open_price)
        self.assertEqual(3, position.event_count())
        self.assertEqual([order1.cl_ord_id, order2.cl_ord_id], position.get_order_ids())
        self.assertEqual(UNIX_EPOCH, position.closed_time)
        self.assertEqual(1.00003, position.avg_close_price)
        self.assertFalse(position.is_long())
        self.assertFalse(position.is_short())
        self.assertTrue(position.is_closed())
        self.assertEqual(-2.999999999997449e-05, position.realized_points)
        self.assertEqual(-2.999999999997449e-05, position.realized_return)
        self.assertEqual(Money(-3.000, Currency.USD), position.realized_pnl)
        self.assertEqual(0.0, position.unrealized_points(last))
        self.assertEqual(0.0, position.unrealized_return(last))
        self.assertEqual(Money(00, Currency.USD), position.unrealized_pnl(last))
        self.assertEqual(-2.999999999997449e-05, position.total_points(last))
        self.assertEqual(-2.999999999997449e-05, position.total_return(last))
        self.assertEqual(Money(-3.000, Currency.USD), position.total_pnl(last))

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
            fill_price=Price(1.00000, 5),
        )

        last = QuoteTick(
            AUDUSD_FXCM,
            Price(1.00050, 5),
            Price(1.00048, 5),
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
        self.assertEqual(1.0, position.avg_open_price)
        self.assertEqual(2, position.event_count())
        self.assertEqual([order1.cl_ord_id, order2.cl_ord_id], position.get_order_ids())
        self.assertEqual([
            ExecutionId("E-19700101-000000-001-001-1"),
            ExecutionId("E-19700101-000000-001-001-2")
        ],
            position.get_execution_ids(),
        )
        self.assertEqual(UNIX_EPOCH, position.closed_time)
        self.assertEqual(1.0, position.avg_close_price)
        self.assertFalse(position.is_long())
        self.assertFalse(position.is_short())
        self.assertTrue(position.is_closed())
        self.assertEqual(0.0, position.realized_points)
        self.assertEqual(0.0, position.realized_return)
        self.assertEqual(Money(00, Currency.USD), position.realized_pnl)
        self.assertEqual(0.0, position.unrealized_points(last))
        self.assertEqual(0.0, position.unrealized_return(last))
        self.assertEqual(Money(00, Currency.USD), position.unrealized_pnl(last))
        self.assertEqual(0.0, position.total_points(last))
        self.assertEqual(0.0, position.total_return(last))
        self.assertEqual(Money(00, Currency.USD), position.total_pnl(last))

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

        fill1 = TestStubs.event_order_filled(order1, PositionId("P-123456"))
        fill2 = TestStubs.event_order_filled(order2, PositionId("P-123456"), fill_price=Price(1.00001, 5))
        fill3 = TestStubs.event_order_filled(order3, PositionId("P-123456"), fill_price=Price(1.00010, 5))

        last = QuoteTick(
            AUDUSD_FXCM,
            Price(1.00050, 5),
            Price(1.00048, 5),
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
        self.assertEqual(1.000005, position.avg_open_price)
        self.assertEqual(3, position.event_count())
        self.assertEqual([order1.cl_ord_id, order2.cl_ord_id, order3.cl_ord_id], position.get_order_ids())
        self.assertEqual(UNIX_EPOCH, position.closed_time)
        self.assertEqual(1.0001, position.avg_close_price)
        self.assertFalse(position.is_long())
        self.assertFalse(position.is_short())
        self.assertTrue(position.is_closed())
        self.assertEqual(9.499999999995623e-05, position.realized_points)
        self.assertEqual(9.499952500233122e-05, position.realized_return)
        self.assertEqual(Money(19.000, Currency.USD), position.realized_pnl)
        self.assertEqual(0.0, position.unrealized_points(last))
        self.assertEqual(0.0, position.unrealized_return(last))
        self.assertEqual(Money(00, Currency.USD), position.unrealized_pnl(last))
        self.assertEqual(9.499999999995623e-05, position.total_points(last))
        self.assertEqual(9.499952500233122e-05, position.total_return(last))
        self.assertEqual(Money(19.000, Currency.USD), position.total_pnl(last))

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

from nautilus_trader.backtest.loaders import InstrumentLoader
from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.factories import OrderFactory
from nautilus_trader.core.decimal import Decimal
from nautilus_trader.core.uuid import uuid4
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


AUDUSD_FXCM = InstrumentLoader.default_fx_ccy(TestStubs.symbol_audusd_fxcm())
BTCUSDT_BINANCE = InstrumentLoader.btcusdt_binance()
ETHUSDT_BINANCE = InstrumentLoader.ethusdt_binance()
XBTUSD_BITMEX = InstrumentLoader.xbtusd_bitmex()
ETHUSD_BITMEX = InstrumentLoader.ethusd_bitmex()


class PositionTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        self.account_id = TestStubs.account_id()
        self.order_factory = OrderFactory(
            trader_id=TraderId("TESTER", "000"),
            strategy_id=StrategyId("S", "001"),
            clock=TestClock(),
        )

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
            AUDUSD_FXCM.symbol,
            OrderSide.BUY,
            Quantity(100000),
        )

        fill = TestStubs.event_order_filled(
            order,
            instrument=AUDUSD_FXCM,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S", "001"),
            fill_price=Price("1.00001"),
        )

        last = QuoteTick(
            AUDUSD_FXCM.symbol,
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
        self.assertEqual([order.cl_ord_id], position.cl_ord_ids)
        self.assertEqual([ExecutionId("E-19700101-000000-000-001-1")], position.execution_ids)
        self.assertEqual(ExecutionId("E-19700101-000000-000-001-1"), position.last_execution_id)
        self.assertEqual(PositionId("P-123456"), position.id)
        self.assertTrue(position.is_long)
        self.assertFalse(position.is_short)
        self.assertFalse(position.is_closed)
        self.assertEqual(0, position.realized_points)
        self.assertEqual(0, position.realized_return)
        self.assertEqual(Money(-2.00, USD), position.realized_pnl)
        self.assertEqual(Money(49.00, USD), position.unrealized_pnl(last))
        self.assertEqual(Money(47.00, USD), position.total_pnl(last))
        self.assertEqual("Position(id=P-123456, LONG 100,000 AUD/USD.FXCM)", repr(position))

    def test_position_filled_with_sell_order_returns_expected_attributes(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_FXCM.symbol,
            OrderSide.SELL,
            Quantity(100000),
        )

        fill = TestStubs.event_order_filled(
            order,
            instrument=AUDUSD_FXCM,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S", "001"),
            fill_price=Price("1.00001"),
        )

        last = QuoteTick(
            AUDUSD_FXCM.symbol,
            Price("1.00048"),
            Price("1.00050"),
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
        self.assertEqual([ExecutionId("E-19700101-000000-000-001-1")], position.execution_ids)
        self.assertEqual(ExecutionId("E-19700101-000000-000-001-1"), position.last_execution_id)
        self.assertEqual(PositionId("P-123456"), position.id)
        self.assertFalse(position.is_long)
        self.assertTrue(position.is_short)
        self.assertFalse(position.is_closed)
        self.assertEqual(0, position.realized_points)
        self.assertEqual(0, position.realized_return)
        self.assertEqual(Money(-2.00, USD), position.realized_pnl)
        self.assertEqual(Money(-49.00, USD), position.unrealized_pnl(last))
        self.assertEqual(Money(-51.00, USD), position.total_pnl(last))
        self.assertEqual("Position(id=P-123456, SHORT 100,000 AUD/USD.FXCM)", repr(position))

    def test_position_partial_fills_with_buy_order_returns_expected_attributes(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_FXCM.symbol,
            OrderSide.BUY,
            Quantity(100000),
        )

        fill = TestStubs.event_order_filled(
            order,
            instrument=AUDUSD_FXCM,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S", "001"),
            fill_price=Price("1.00001"),
            filled_qty=Quantity(50000),
            leaves_qty=Quantity(50000),
        )

        last = QuoteTick(
            AUDUSD_FXCM.symbol,
            Price("1.00048"),
            Price("1.00050"),
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
        self.assertEqual(Money(-2.00, USD), position.realized_pnl)
        self.assertEqual(Money(23.50, USD), position.unrealized_pnl(last))
        self.assertEqual(Money(21.50, USD), position.total_pnl(last))
        self.assertEqual("Position(id=P-123456, LONG 50,000 AUD/USD.FXCM)", repr(position))

    def test_position_partial_fills_with_sell_order_returns_expected_attributes(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_FXCM.symbol,
            OrderSide.SELL,
            Quantity(100000),
        )

        fill1 = TestStubs.event_order_filled(
            order,
            instrument=AUDUSD_FXCM,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S", "001"),
            fill_price=Price("1.00001"),
            filled_qty=Quantity(50000),
            leaves_qty=Quantity(50000),
        )

        fill2 = TestStubs.event_order_filled(
            order,
            instrument=AUDUSD_FXCM,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S", "001"),
            fill_price=Price("1.00002"),
            filled_qty=Quantity(50000),
            leaves_qty=Quantity(),
        )

        position = Position(fill1)

        last = QuoteTick(
            AUDUSD_FXCM.symbol,
            Price("1.00048"),
            Price("1.00050"),
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
        self.assertEqual(Money(-4.00, USD), position.realized_pnl)
        self.assertEqual(Money(-48.50, USD), position.unrealized_pnl(last))
        self.assertEqual(Money(-52.50, USD), position.total_pnl(last))
        self.assertEqual("Position(id=P-123456, SHORT 100,000 AUD/USD.FXCM)", repr(position))

    def test_position_filled_with_buy_order_then_sell_order_returns_expected_attributes(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_FXCM.symbol,
            OrderSide.BUY,
            Quantity(150000),
        )

        fill1 = TestStubs.event_order_filled(
            order,
            instrument=AUDUSD_FXCM,
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
            Price("1.00011"),
            Money(0, USD),
            LiquiditySide.TAKER,
            AUDUSD_FXCM.get_cost_spec(),
            UNIX_EPOCH + timedelta(minutes=1),
            uuid4(),
            UNIX_EPOCH,
        )

        last = QuoteTick(
            AUDUSD_FXCM.symbol,
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
        self.assertEqual(Decimal("1.00011"), position.avg_close)
        self.assertFalse(position.is_long)
        self.assertFalse(position.is_short)
        self.assertTrue(position.is_closed)
        self.assertEqual(Decimal("0.00010"), position.realized_points)
        self.assertEqual(Decimal('0.00009999900000999990000099999000'), position.realized_return)
        self.assertEqual(Money(12.00, USD), position.realized_pnl)
        self.assertEqual(Money(0, USD), position.unrealized_pnl(last))
        self.assertEqual(Money(12.00, USD), position.total_pnl(last))
        self.assertEqual("Position(id=P-123456, FLAT AUD/USD.FXCM)", repr(position))

    def test_position_filled_with_sell_order_then_buy_order_returns_expected_attributes(self):
        # Arrange
        order1 = self.order_factory.market(
            AUDUSD_FXCM.symbol,
            OrderSide.SELL,
            Quantity(100000),
        )

        order2 = self.order_factory.market(
            AUDUSD_FXCM.symbol,
            OrderSide.BUY,
            Quantity(100000),
        )

        fill1 = TestStubs.event_order_filled(order1, instrument=AUDUSD_FXCM)

        position = Position(fill1)

        fill2 = TestStubs.event_order_filled(
            order2,
            instrument=AUDUSD_FXCM,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S", "001"),
            fill_price=Price("1.00001"),
            filled_qty=Quantity(50000),
            leaves_qty=Quantity(50000),
        )

        fill3 = TestStubs.event_order_filled(
            order2,
            instrument=AUDUSD_FXCM,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S", "001"),
            fill_price=Price("1.00003"),
            filled_qty=Quantity(50000),
            leaves_qty=Quantity(0),
        )

        last = QuoteTick(
            AUDUSD_FXCM.symbol,
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
        self.assertEqual([order1.cl_ord_id, order2.cl_ord_id], position.cl_ord_ids)
        self.assertEqual(UNIX_EPOCH, position.closed_time)
        self.assertEqual(Decimal("1.00002"), position.avg_close)
        self.assertFalse(position.is_long)
        self.assertFalse(position.is_short)
        self.assertTrue(position.is_closed)
        self.assertEqual(Money(-8.00, USD), position.realized_pnl)
        self.assertEqual(Money(0, USD), position.unrealized_pnl(last))
        self.assertEqual(Money(-8.000, USD), position.total_pnl(last))
        self.assertEqual("Position(id=O-19700101-000000-000-001-1, FLAT AUD/USD.FXCM)", repr(position))

    def test_position_filled_with_no_change_returns_expected_attributes(self):
        # Arrange
        order1 = self.order_factory.market(
            AUDUSD_FXCM.symbol,
            OrderSide.BUY,
            Quantity(100000),
        )

        order2 = self.order_factory.market(
            AUDUSD_FXCM.symbol,
            OrderSide.SELL,
            Quantity(100000),
        )

        fill1 = TestStubs.event_order_filled(order1,  instrument=AUDUSD_FXCM)

        position = Position(fill1)

        fill2 = TestStubs.event_order_filled(
            order2,
            instrument=AUDUSD_FXCM,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S", "001"),
            fill_price=Price("1.00000"),
        )

        last = QuoteTick(
            AUDUSD_FXCM.symbol,
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
        self.assertEqual([order1.cl_ord_id, order2.cl_ord_id], position.cl_ord_ids)
        self.assertEqual([
            ExecutionId("E-19700101-000000-000-001-1"),
            ExecutionId("E-19700101-000000-000-001-2")
        ],
            position.execution_ids,
        )
        self.assertEqual(UNIX_EPOCH, position.closed_time)
        self.assertEqual(Decimal("1.0"), position.avg_close)
        self.assertFalse(position.is_long)
        self.assertFalse(position.is_short)
        self.assertTrue(position.is_closed)
        self.assertEqual(0, position.realized_points)
        self.assertEqual(0, position.realized_return)
        self.assertEqual(Money(-4.00, USD), position.realized_pnl)
        self.assertEqual(Money(0, USD), position.unrealized_pnl(last))
        self.assertEqual(Money(-4.00, USD), position.total_pnl(last))
        self.assertEqual("Position(id=O-19700101-000000-000-001-1, FLAT AUD/USD.FXCM)", repr(position))

    def test_position_long_with_multiple_filled_orders_returns_expected_attributes(self):
        # Arrange
        order1 = self.order_factory.market(
            AUDUSD_FXCM.symbol,
            OrderSide.BUY,
            Quantity(100000),
        )

        order2 = self.order_factory.market(
            AUDUSD_FXCM.symbol,
            OrderSide.BUY,
            Quantity(100000),
        )

        order3 = self.order_factory.market(
            AUDUSD_FXCM.symbol,
            OrderSide.SELL,
            Quantity(200000),
        )

        fill1 = TestStubs.event_order_filled(
            order1,
            instrument=AUDUSD_FXCM,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S", "001"),
        )

        fill2 = TestStubs.event_order_filled(
            order2,
            instrument=AUDUSD_FXCM,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S", "001"),
            fill_price=Price("1.00001"),
        )

        fill3 = TestStubs.event_order_filled(
            order3,
            instrument=AUDUSD_FXCM,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S", "001"),
            fill_price=Price("1.00010"),
        )

        last = QuoteTick(
            AUDUSD_FXCM.symbol,
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
        self.assertEqual([order1.cl_ord_id, order2.cl_ord_id, order3.cl_ord_id], position.cl_ord_ids)
        self.assertEqual(UNIX_EPOCH, position.closed_time)
        self.assertEqual(Decimal("1.0001"), position.avg_close)
        self.assertFalse(position.is_long)
        self.assertFalse(position.is_short)
        self.assertTrue(position.is_closed)
        self.assertEqual(Money(11.00, USD), position.realized_pnl)
        self.assertEqual(Money(0, USD), position.unrealized_pnl(last))
        self.assertEqual(Money(11.00, USD), position.total_pnl(last))
        self.assertEqual("Position(id=P-123456, FLAT AUD/USD.FXCM)", repr(position))

    def test_pnl_calculation_from_trading_technologies_example(self):
        # https://www.tradingtechnologies.com/xtrader-help/fix-adapter-reference/pl-calculation-algorithm/understanding-pl-calculations/  # noqa

        # Arrange
        order1 = self.order_factory.market(
            ETHUSDT_BINANCE.symbol,
            OrderSide.BUY,
            Quantity(12),
        )

        order2 = self.order_factory.market(
            ETHUSDT_BINANCE.symbol,
            OrderSide.BUY,
            Quantity(17),
        )

        order3 = self.order_factory.market(
            ETHUSDT_BINANCE.symbol,
            OrderSide.SELL,
            Quantity(9),
        )

        order4 = self.order_factory.market(
            ETHUSDT_BINANCE.symbol,
            OrderSide.SELL,
            Quantity(4),
        )

        order5 = self.order_factory.market(
            ETHUSDT_BINANCE.symbol,
            OrderSide.BUY,
            Quantity(3),
        )

        # Act
        fill1 = TestStubs.event_order_filled(
            order1,
            instrument=BTCUSDT_BINANCE,
            fill_price=Price(100),
        )

        position = Position(fill1)

        fill2 = TestStubs.event_order_filled(
            order2,
            instrument=BTCUSDT_BINANCE,
            fill_price=Price(99),
        )

        position.apply(fill2)
        self.assertEqual(Quantity(29), position.quantity)
        self.assertEqual(Money(-2.88300000, USDT), position.realized_pnl)
        self.assertEqual(99.41379310344827, position.avg_open.as_double())

        fill3 = TestStubs.event_order_filled(
            order3,
            instrument=BTCUSDT_BINANCE,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S", "001"),
            fill_price=Price(101),
        )

        position.apply(fill3)
        self.assertEqual(Quantity(20), position.quantity)
        self.assertEqual(Money(10.48386207, USDT), position.realized_pnl)
        self.assertEqual(99.41379310344827, position.avg_open.as_double())

        fill4 = TestStubs.event_order_filled(
            order4,
            instrument=BTCUSDT_BINANCE,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S", "001"),
            fill_price=Price(105),
        )

        position.apply(fill4)
        self.assertEqual(Quantity(16), position.quantity)
        self.assertEqual(Money(32.40868966, USDT), position.realized_pnl)
        self.assertEqual(99.41379310344827, position.avg_open.as_double())

        fill5 = TestStubs.event_order_filled(
            order5,
            instrument=BTCUSDT_BINANCE,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S", "001"),
            fill_price=Price(103),
        )

        position.apply(fill5)
        self.assertEqual(Quantity(19), position.quantity)
        self.assertEqual(Money(32.09968966, USDT), position.realized_pnl)
        self.assertEqual(99.98003629764065, position.avg_open.as_double())
        self.assertEqual("Position(id=O-19700101-000000-000-001-1, LONG 19 ETH/USDT.BINANCE)", repr(position))

    def test_position_realised_pnl_with_interleaved_order_sides(self):
        # Arrange
        order1 = self.order_factory.market(
            BTCUSDT_BINANCE.symbol,
            OrderSide.BUY,
            Quantity("12.000000"),
        )

        order2 = self.order_factory.market(
            BTCUSDT_BINANCE.symbol,
            OrderSide.BUY,
            Quantity("17.000000"),
        )

        order3 = self.order_factory.market(
            BTCUSDT_BINANCE.symbol,
            OrderSide.SELL,
            Quantity("9.000000"),
        )

        order4 = self.order_factory.market(
            BTCUSDT_BINANCE.symbol,
            OrderSide.BUY,
            Quantity("3.000000"),
        )

        order5 = self.order_factory.market(
            BTCUSDT_BINANCE.symbol,
            OrderSide.SELL,
            Quantity("4.000000"),
        )

        # Act
        fill1 = TestStubs.event_order_filled(
            order1,
            instrument=BTCUSDT_BINANCE,
            fill_price=Price("10000.00"),
        )

        position = Position(fill1)

        fill2 = TestStubs.event_order_filled(
            order2,
            instrument=BTCUSDT_BINANCE,
            fill_price=Price("9999.00"),
        )

        position.apply(fill2)
        self.assertEqual(Quantity("29.000000"), position.quantity)
        self.assertEqual(Money(-289.98300000, USDT), position.realized_pnl)
        self.assertEqual(9999.413793103448275862068966, position.avg_open.as_double())

        fill3 = TestStubs.event_order_filled(
            order3,
            instrument=BTCUSDT_BINANCE,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S", "001"),
            fill_price=Price("10001.00"),
        )

        position.apply(fill3)
        self.assertEqual(Quantity(20), position.quantity)
        self.assertEqual(Money(-365.71613793, USDT), position.realized_pnl)
        self.assertEqual(9999.413793103448275862068966, position.avg_open.as_double())

        fill4 = TestStubs.event_order_filled(
            order4,
            instrument=BTCUSDT_BINANCE,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S", "001"),
            fill_price=Price("10003.00"),
        )

        position.apply(fill4)
        self.assertEqual(Quantity(23), position.quantity)
        self.assertEqual(Money(-395.72513793, USDT), position.realized_pnl)
        self.assertEqual(9999.88155922039, position.avg_open.as_double())

        fill5 = TestStubs.event_order_filled(
            order5,
            instrument=BTCUSDT_BINANCE,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S", "001"),
            fill_price=Price("10005"),
        )

        position.apply(fill5)
        self.assertEqual(Quantity(19), position.quantity)
        self.assertEqual(Money(-415.27137481, USDT), position.realized_pnl)
        self.assertEqual(9999.88155922039, position.avg_open.as_double())
        self.assertEqual("Position(id=O-19700101-000000-000-001-1, LONG 19.000000 BTC/USDT.BINANCE)", repr(position))

    def test_calculate_pnl_for_long_position_win(self):
        # Arrange
        order = self.order_factory.market(
            BTCUSDT_BINANCE.symbol,
            OrderSide.BUY,
            Quantity(12),
        )

        fill = TestStubs.event_order_filled(
            order,
            instrument=BTCUSDT_BINANCE,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S", "001"),
            fill_price=Price("10500.00"),
        )
        position = Position(fill)

        # Act
        pnl = position.calculate_pnl(
            avg_open=Price("10500.00"),
            avg_close=Price("10510.00"),
            quantity=Quantity(12),
        )

        # Assert
        self.assertEqual(Money(120.00000000, USDT), pnl)
        self.assertEqual(Money(-126.00000000, USDT), position.realized_pnl)
        self.assertEqual(Money(-126.00000000, USDT), position.commissions)

    def test_calculate_pnl_for_long_position_loss(self):
        # Arrange
        order = self.order_factory.market(
            BTCUSDT_BINANCE.symbol,
            OrderSide.BUY,
            Quantity(12),
        )

        fill = TestStubs.event_order_filled(
            order,
            instrument=BTCUSDT_BINANCE,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S", "001"),
            fill_price=Price("10500.00"),
        )

        position = Position(fill)

        # Act
        pnl = position.calculate_pnl(
            Price("10500.00"),
            Price("10480.50"),
            Quantity(10),
        )

        # Assert
        self.assertEqual(Money(-195.00000000, USDT), pnl)
        self.assertEqual(Money(-126.00000000, USDT), position.realized_pnl)
        self.assertEqual(Money(-126.00000000, USDT), position.commissions)

    def test_calculate_pnl_for_short_position_win(self):
        # Arrange
        order = self.order_factory.market(
            BTCUSDT_BINANCE.symbol,
            OrderSide.SELL,
            Quantity("10.150000"),
        )

        fill = TestStubs.event_order_filled(
            order,
            instrument=BTCUSDT_BINANCE,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S", "001"),
            fill_price=Price("10500.00"),
        )

        position = Position(fill)

        # Act
        pnl = position.calculate_pnl(
            Price("10500.00"),
            Price("10390.00"),
            Quantity("10.150000"),
        )

        # Assert
        self.assertEqual(Money(1116.50000000, USDT), pnl)
        self.assertEqual(Money(-106.57500000, USDT), position.realized_pnl)
        self.assertEqual(Money(-106.57500000, USDT), position.commissions)

    def test_calculate_pnl_for_short_position_loss(self):
        # Arrange
        order = self.order_factory.market(
            BTCUSDT_BINANCE.symbol,
            OrderSide.SELL,
            Quantity("10"),
        )

        fill = TestStubs.event_order_filled(
            order,
            instrument=BTCUSDT_BINANCE,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S", "001"),
            fill_price=Price("10500.00"),
        )

        position = Position(fill)

        # Act
        pnl = position.calculate_pnl(
            Price("10500.00"),
            Price("10670.50"),
            Quantity("10.000000"),
        )

        # Assert
        self.assertEqual(Money(-1705.00000000, USDT), pnl)
        self.assertEqual(Money(-105.00000000, USDT), position.realized_pnl)
        self.assertEqual(Money(-105.00000000, USDT), position.commissions)

    def test_calculate_pnl_for_inverse1(self):
        # Arrange
        order = self.order_factory.market(
            XBTUSD_BITMEX.symbol,
            OrderSide.SELL,
            Quantity(100000),
        )

        fill = TestStubs.event_order_filled(
            order,
            instrument=XBTUSD_BITMEX,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S", "001"),
            fill_price=Price("10500.00"),
        )

        position = Position(fill)

        # Act
        pnl = position.calculate_pnl(
            Price("10500.00"),
            Price("10670.50"),
            Quantity(100000),
        )

        # Assert
        self.assertEqual(Money(-0.15217745, BTC), pnl)
        self.assertEqual(Money(-0.00714286, BTC), position.realized_pnl)
        self.assertEqual(Money(-0.00714286, BTC), position.commissions)

    def test_calculate_pnl_for_inverse2(self):
        # Arrange
        order = self.order_factory.market(
            ETHUSD_BITMEX.symbol,
            OrderSide.SELL,
            Quantity(100000),
        )

        fill = TestStubs.event_order_filled(
            order,
            instrument=ETHUSD_BITMEX,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S", "001"),
            fill_price=Price("375.95"),
            xrate=Decimal("0.0294337")
        )

        position = Position(fill)

        # Act
        pnl = position.calculate_pnl(
            Price("375.95"),
            Price("365.50"),
            Quantity(100000),
            xrate=Decimal("0.0294337")
        )

        # Assert
        self.assertEqual(Money(0.22384308, BTC), pnl)
        self.assertEqual(Money(-0.00587186, BTC), position.realized_pnl)
        self.assertEqual(Money(-0.00587186, BTC), position.commissions)

    def test_calculate_unrealized_pnl_for_long(self):
        # Arrange
        order1 = self.order_factory.market(
            BTCUSDT_BINANCE.symbol,
            OrderSide.BUY,
            Quantity("2.000000"),
        )

        order2 = self.order_factory.market(
            BTCUSDT_BINANCE.symbol,
            OrderSide.BUY,
            Quantity("2.000000"),
        )

        fill1 = TestStubs.event_order_filled(
            order1,
            instrument=BTCUSDT_BINANCE,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S", "001"),
            fill_price=Price("10500.00"),
        )

        fill2 = TestStubs.event_order_filled(
            order2,
            instrument=BTCUSDT_BINANCE,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S", "001"),
            fill_price=Price("10500.00"),
        )

        position = Position(fill1)
        position.apply(fill2)

        last = QuoteTick(
            BTCUSDT_BINANCE.symbol,
            Price("11505.60"),
            Price("11506.65"),
            Quantity(20),
            Quantity(20),
            UNIX_EPOCH,
        )

        # Act
        pnl = position.unrealized_pnl(last)

        # Assert
        self.assertEqual(Money(4022.40000000, USDT), pnl)
        self.assertEqual(Money(-42.00000000, USDT), position.realized_pnl)
        self.assertEqual(Money(-42.00000000, USDT), position.commissions)

    def test_calculate_unrealized_pnl_for_short(self):
        # Arrange
        order = self.order_factory.market(
            BTCUSDT_BINANCE.symbol,
            OrderSide.SELL,
            Quantity("5.912000"),
        )

        fill = TestStubs.event_order_filled(
            order,
            instrument=BTCUSDT_BINANCE,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S", "001"),
            fill_price=Price("10505.60"),
        )

        position = Position(fill)

        last = QuoteTick(
            BTCUSDT_BINANCE.symbol,
            Price("10405.60"),
            Price("10407.15"),
            Quantity("20.000000"),
            Quantity("20.000000"),
            UNIX_EPOCH,
        )

        pnl = position.unrealized_pnl(last)

        # Assert
        self.assertEqual(Money(582.03640000, USDT), pnl)
        self.assertEqual(Money(-62.10910720, USDT), position.realized_pnl)
        self.assertEqual(Money(-62.10910720, USDT), position.commissions)

    def test_calculate_unrealized_pnl_for_long_inverse(self):
        # Arrange
        order = self.order_factory.market(
            XBTUSD_BITMEX.symbol,
            OrderSide.BUY,
            Quantity(100000),
        )

        fill = TestStubs.event_order_filled(
            order,
            instrument=XBTUSD_BITMEX,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S", "001"),
            fill_price=Price("10500.00"),
        )

        position = Position(fill)

        last = QuoteTick(
            XBTUSD_BITMEX.symbol,
            Price("11505.60"),
            Price("11506.65"),
            Quantity(25000),
            Quantity(25000),
            UNIX_EPOCH,
        )

        # Act

        pnl = position.unrealized_pnl(last)

        # Assert
        self.assertEqual(Money(0.83238969, BTC), pnl)
        self.assertEqual(Money(-0.00714286, BTC), position.realized_pnl)
        self.assertEqual(Money(-0.00714286, BTC), position.commissions)

    def test_calculate_unrealized_pnl_for_short_inverse(self):
        # Arrange
        order = self.order_factory.market(
            XBTUSD_BITMEX.symbol,
            OrderSide.SELL,
            Quantity(1250000),
        )

        fill = TestStubs.event_order_filled(
            order,
            instrument=XBTUSD_BITMEX,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S", "001"),
            fill_price=Price("15500.00"),
        )

        position = Position(fill)

        last = QuoteTick(
            XBTUSD_BITMEX.symbol,
            Price("12505.60"),
            Price("12506.65"),
            Quantity(125000),
            Quantity(125000),
            UNIX_EPOCH,
        )

        # Act

        pnl = position.unrealized_pnl(last)

        # Assert
        self.assertEqual(Money(19.30166700, BTC), pnl)
        self.assertEqual(Money(-0.06048387, BTC), position.realized_pnl)
        self.assertEqual(Money(-0.06048387, BTC), position.commissions)

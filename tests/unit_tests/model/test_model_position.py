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
from tests.test_kit.providers import TestInstrumentProvider
from tests.test_kit.stubs import TestStubs


AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")
BTCUSDT_BINANCE = TestInstrumentProvider.btcusdt_binance()
ETHUSDT_BINANCE = TestInstrumentProvider.ethusdt_binance()
XBTUSD_BITMEX = TestInstrumentProvider.xbtusd_bitmex()
ETHUSD_BITMEX = TestInstrumentProvider.ethusd_bitmex()


class PositionTests(unittest.TestCase):
    def setUp(self):
        # Fixture Setup
        self.account_id = TestStubs.account_id()
        self.order_factory = OrderFactory(
            trader_id=TraderId("TESTER", "000"),
            strategy_id=StrategyId("S", "001"),
            clock=TestClock(),
        )

    def test_side_from_order_side_given_invalid_value_returns_none(self):
        # Arrange
        # Act
        self.assertRaises(ValueError, Position.side_from_order_side, 0)

    @parameterized.expand(
        [
            [OrderSide.BUY, PositionSide.LONG],
            [OrderSide.SELL, PositionSide.SHORT],
        ]
    )
    def test_side_from_order_side_given_valid_sides_returns_expected_side(
        self, order_side, expected
    ):
        # Arrange
        # Act
        position_side = Position.side_from_order_side(order_side)

        # Assert
        self.assertEqual(expected, position_side)

    def test_position_filled_with_buy_order_returns_expected_attributes(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity(100000),
        )

        fill = TestStubs.event_order_filled(
            order,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S", "001"),
            last_px=Price("1.00001"),
        )

        last = Price("1.00050")

        # Act
        position = Position(fill=fill)

        # Assert
        self.assertFalse(position != position)  # Equality operator test
        self.assertEqual(
            ClientOrderId("O-19700101-000000-000-001-1"), position.from_order
        )
        self.assertEqual(Quantity(100000), position.quantity)
        self.assertEqual(Quantity(100000), position.peak_qty)
        self.assertEqual(OrderSide.BUY, position.entry)
        self.assertEqual(PositionSide.LONG, position.side)
        self.assertEqual(0, position.opened_timestamp_ns)
        self.assertEqual(0, position.open_duration_ns)
        self.assertEqual(Decimal("1.00001"), position.avg_px_open)
        self.assertEqual(1, position.event_count)
        self.assertEqual([order.cl_ord_id], position.cl_ord_ids)
        self.assertEqual([OrderId("1")], position.order_ids)
        self.assertEqual(
            [ExecutionId("E-19700101-000000-000-001-1")], position.execution_ids
        )
        self.assertEqual(
            ExecutionId("E-19700101-000000-000-001-1"), position.last_execution_id
        )
        self.assertEqual(PositionId("P-123456"), position.id)
        self.assertEqual(1, len(position.events))
        self.assertTrue(position.is_long)
        self.assertFalse(position.is_short)
        self.assertTrue(position.is_open)
        self.assertFalse(position.is_closed)
        self.assertEqual(0, position.realized_points)
        self.assertEqual(0, position.realized_return)
        self.assertEqual(Money(-2.00, USD), position.realized_pnl)
        self.assertEqual(Money(49.00, USD), position.unrealized_pnl(last))
        self.assertEqual(Money(47.00, USD), position.total_pnl(last))
        self.assertEqual(Money(2.00, USD), position.commission)
        self.assertEqual([Money(2.00, USD)], position.commissions())
        self.assertEqual(
            "Position(LONG 100,000 AUD/USD.SIM, id=P-123456)", repr(position)
        )

    def test_position_filled_with_sell_order_returns_expected_attributes(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.SELL,
            Quantity(100000),
        )

        fill = TestStubs.event_order_filled(
            order,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S", "001"),
            last_px=Price("1.00001"),
        )

        last = Price("1.00050")

        # Act
        position = Position(fill=fill)

        # Assert
        self.assertEqual(Quantity(100000), position.quantity)
        self.assertEqual(Quantity(100000), position.peak_qty)
        self.assertEqual(PositionSide.SHORT, position.side)
        self.assertEqual(0, position.opened_timestamp_ns)
        self.assertEqual(Decimal("1.00001"), position.avg_px_open)
        self.assertEqual(1, position.event_count)
        self.assertEqual(
            [ExecutionId("E-19700101-000000-000-001-1")], position.execution_ids
        )
        self.assertEqual(
            ExecutionId("E-19700101-000000-000-001-1"), position.last_execution_id
        )
        self.assertEqual(PositionId("P-123456"), position.id)
        self.assertFalse(position.is_long)
        self.assertTrue(position.is_short)
        self.assertTrue(position.is_open)
        self.assertFalse(position.is_closed)
        self.assertEqual(0, position.realized_points)
        self.assertEqual(0, position.realized_return)
        self.assertEqual(Money(-2.00, USD), position.realized_pnl)
        self.assertEqual(Money(-49.00, USD), position.unrealized_pnl(last))
        self.assertEqual(Money(-51.00, USD), position.total_pnl(last))
        self.assertEqual(Money(2.00, USD), position.commission)
        self.assertEqual([Money(2.00, USD)], position.commissions())
        self.assertEqual(
            "Position(SHORT 100,000 AUD/USD.SIM, id=P-123456)", repr(position)
        )

    def test_position_partial_fills_with_buy_order_returns_expected_attributes(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity(100000),
        )

        fill = TestStubs.event_order_filled(
            order,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S", "001"),
            last_px=Price("1.00001"),
            last_qty=Quantity(50000),
        )

        last = Price("1.00048")

        position = Position(fill=fill)

        # Act
        # Assert
        self.assertEqual(Quantity(50000), position.quantity)
        self.assertEqual(Quantity(50000), position.peak_qty)
        self.assertEqual(PositionSide.LONG, position.side)
        self.assertEqual(0, position.opened_timestamp_ns)
        self.assertEqual(Decimal("1.00001"), position.avg_px_open)
        self.assertEqual(1, position.event_count)
        self.assertTrue(position.is_long)
        self.assertFalse(position.is_short)
        self.assertTrue(position.is_open)
        self.assertFalse(position.is_closed)
        self.assertEqual(0, position.realized_points)
        self.assertEqual(0, position.realized_return)
        self.assertEqual(Money(-2.00, USD), position.realized_pnl)
        self.assertEqual(Money(23.50, USD), position.unrealized_pnl(last))
        self.assertEqual(Money(21.50, USD), position.total_pnl(last))
        self.assertEqual(Money(2.00, USD), position.commission)
        self.assertEqual([Money(2.00, USD)], position.commissions())
        self.assertEqual(
            "Position(LONG 50,000 AUD/USD.SIM, id=P-123456)", repr(position)
        )

    def test_position_partial_fills_with_sell_order_returns_expected_attributes(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.SELL,
            Quantity(100000),
        )

        fill1 = TestStubs.event_order_filled(
            order,
            instrument=AUDUSD_SIM,
            execution_id=ExecutionId("1"),
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S", "001"),
            last_px=Price("1.00001"),
            last_qty=Quantity(50000),
        )

        fill2 = TestStubs.event_order_filled(
            order,
            instrument=AUDUSD_SIM,
            execution_id=ExecutionId("2"),
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S", "001"),
            last_px=Price("1.00002"),
            last_qty=Quantity(50000),
        )

        position = Position(fill=fill1)

        last = Price("1.00050")

        # Act
        position.apply(fill2)

        # Assert
        self.assertEqual(Quantity(100000), position.quantity)
        self.assertEqual(PositionSide.SHORT, position.side)
        self.assertEqual(0, position.opened_timestamp_ns)
        self.assertEqual(Decimal("1.000015"), position.avg_px_open)
        self.assertEqual(2, position.event_count)
        self.assertFalse(position.is_long)
        self.assertTrue(position.is_short)
        self.assertTrue(position.is_open)
        self.assertFalse(position.is_closed)
        self.assertEqual(0, position.realized_points)
        self.assertEqual(0, position.realized_return)
        self.assertEqual(Money(-4.00, USD), position.realized_pnl)
        self.assertEqual(Money(-48.50, USD), position.unrealized_pnl(last))
        self.assertEqual(Money(-52.50, USD), position.total_pnl(last))
        self.assertEqual([Money(4.00, USD)], position.commissions())
        self.assertEqual(Money(4.00, USD), position.commission)
        self.assertEqual(
            "Position(SHORT 100,000 AUD/USD.SIM, id=P-123456)", repr(position)
        )

    def test_position_filled_with_buy_order_then_sell_order_returns_expected_attributes(
        self,
    ):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity(150000),
        )

        fill1 = TestStubs.event_order_filled(
            order,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S", "001"),
            last_px=Price("1.00001"),
            execution_ns=1_000_000_000,
        )

        position = Position(fill=fill1)

        fill2 = OrderFilled(
            self.account_id,
            order.cl_ord_id,
            OrderId("2"),
            ExecutionId("E2"),
            PositionId("T123456"),
            StrategyId("S", "001"),
            order.instrument_id,
            OrderSide.SELL,
            order.quantity,
            Price("1.00011"),
            order.quantity,
            Quantity(),
            AUDUSD_SIM.quote_currency,
            AUDUSD_SIM.is_inverse,
            Money(0, USD),
            LiquiditySide.TAKER,
            2_000_000_000,
            uuid4(),
            0,
        )

        last = Price("1.00050")

        # Act
        position.apply(fill2)

        # Assert
        self.assertEqual(Quantity(), position.quantity)
        self.assertEqual(PositionSide.FLAT, position.side)
        self.assertEqual(1_000_000_000, position.opened_timestamp_ns)
        self.assertEqual(1_000_000_000, position.open_duration_ns)
        self.assertEqual(Decimal("1.00001"), position.avg_px_open)
        self.assertEqual(2, position.event_count)
        self.assertEqual(2_000_000_000, position.closed_timestamp_ns)
        self.assertEqual(Decimal("1.00011"), position.avg_px_close)
        self.assertFalse(position.is_long)
        self.assertFalse(position.is_short)
        self.assertFalse(position.is_open)
        self.assertTrue(position.is_closed)
        self.assertEqual(Decimal("0.00010"), position.realized_points)
        self.assertEqual(
            Decimal("0.00009999900000999990000099999000"), position.realized_return
        )
        self.assertEqual(Money(12.00, USD), position.realized_pnl)
        self.assertEqual(Money(0, USD), position.unrealized_pnl(last))
        self.assertEqual(Money(12.00, USD), position.total_pnl(last))
        self.assertEqual([Money(3.00, USD)], position.commissions())
        self.assertEqual(Money(3.00, USD), position.commission)
        self.assertEqual("Position(FLAT AUD/USD.SIM, id=P-123456)", repr(position))

    def test_position_filled_with_sell_order_then_buy_order_returns_expected_attributes(
        self,
    ):
        # Arrange
        order1 = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.SELL,
            Quantity(100000),
        )

        order2 = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity(100000),
        )

        fill1 = TestStubs.event_order_filled(
            order1,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-19700101-000000-000-001-1"),
        )

        position = Position(fill=fill1)

        fill2 = TestStubs.event_order_filled(
            order2,
            instrument=AUDUSD_SIM,
            execution_id=ExecutionId("1"),
            position_id=PositionId("P-19700101-000000-000-001-1"),
            strategy_id=StrategyId("S", "001"),
            last_px=Price("1.00001"),
            last_qty=Quantity(50000),
        )

        fill3 = TestStubs.event_order_filled(
            order2,
            instrument=AUDUSD_SIM,
            execution_id=ExecutionId("2"),
            position_id=PositionId("P-19700101-000000-000-001-1"),
            strategy_id=StrategyId("S", "001"),
            last_px=Price("1.00003"),
            last_qty=Quantity(50000),
        )

        last = Price("1.00050")

        # Act
        position.apply(fill2)
        position.apply(fill3)

        # Assert
        self.assertEqual(Quantity(), position.quantity)
        self.assertEqual(PositionSide.FLAT, position.side)
        self.assertEqual(0, position.opened_timestamp_ns)
        self.assertEqual(Decimal("1.0"), position.avg_px_open)
        self.assertEqual(3, position.event_count)
        self.assertEqual([order1.cl_ord_id, order2.cl_ord_id], position.cl_ord_ids)
        self.assertEqual(0, position.closed_timestamp_ns)
        self.assertEqual(Decimal("1.00002"), position.avg_px_close)
        self.assertFalse(position.is_long)
        self.assertFalse(position.is_short)
        self.assertFalse(position.is_open)
        self.assertTrue(position.is_closed)
        self.assertEqual(Money(-8.00, USD), position.realized_pnl)
        self.assertEqual(Money(0, USD), position.unrealized_pnl(last))
        self.assertEqual(Money(-8.000, USD), position.total_pnl(last))
        self.assertEqual([Money(6.00, USD)], position.commissions())
        self.assertEqual(Money(6.00, USD), position.commission)
        self.assertEqual(
            "Position(FLAT AUD/USD.SIM, id=P-19700101-000000-000-001-1)", repr(position)
        )

    def test_position_filled_with_no_change_returns_expected_attributes(self):
        # Arrange
        order1 = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity(100000),
        )

        order2 = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.SELL,
            Quantity(100000),
        )

        fill1 = TestStubs.event_order_filled(
            order1,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-19700101-000000-000-001-1"),
        )

        position = Position(fill=fill1)

        fill2 = TestStubs.event_order_filled(
            order2,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-19700101-000000-000-001-1"),
            strategy_id=StrategyId("S", "001"),
            last_px=Price("1.00000"),
        )

        last = Price("1.00050")

        # Act
        position.apply(fill2)

        # Assert
        self.assertEqual(Quantity(), position.quantity)
        self.assertEqual(PositionSide.FLAT, position.side)
        self.assertEqual(0, position.opened_timestamp_ns)
        self.assertEqual(Decimal("1.0"), position.avg_px_open)
        self.assertEqual(2, position.event_count)
        self.assertEqual([order1.cl_ord_id, order2.cl_ord_id], position.cl_ord_ids)
        self.assertEqual(
            [
                ExecutionId("E-19700101-000000-000-001-1"),
                ExecutionId("E-19700101-000000-000-001-2"),
            ],
            position.execution_ids,
        )
        self.assertEqual(0, position.closed_timestamp_ns)
        self.assertEqual(Decimal("1.0"), position.avg_px_close)
        self.assertFalse(position.is_long)
        self.assertFalse(position.is_short)
        self.assertFalse(position.is_open)
        self.assertTrue(position.is_closed)
        self.assertEqual(0, position.realized_points)
        self.assertEqual(0, position.realized_return)
        self.assertEqual(Money(-4.00, USD), position.realized_pnl)
        self.assertEqual(Money(0, USD), position.unrealized_pnl(last))
        self.assertEqual(Money(-4.00, USD), position.total_pnl(last))
        self.assertEqual([Money(4.00, USD)], position.commissions())
        self.assertEqual(Money(4.00, USD), position.commission)
        self.assertEqual(
            "Position(FLAT AUD/USD.SIM, id=P-19700101-000000-000-001-1)", repr(position)
        )

    def test_position_long_with_multiple_filled_orders_returns_expected_attributes(
        self,
    ):
        # Arrange
        order1 = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity(100000),
        )

        order2 = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity(100000),
        )

        order3 = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.SELL,
            Quantity(200000),
        )

        fill1 = TestStubs.event_order_filled(
            order1,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S", "001"),
        )

        fill2 = TestStubs.event_order_filled(
            order2,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S", "001"),
            last_px=Price("1.00001"),
        )

        fill3 = TestStubs.event_order_filled(
            order3,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S", "001"),
            last_px=Price("1.00010"),
        )

        last = Price("1.00050")

        # Act
        position = Position(fill=fill1)
        position.apply(fill2)
        position.apply(fill3)

        # Assert
        self.assertEqual(Quantity(), position.quantity)
        self.assertEqual(PositionSide.FLAT, position.side)
        self.assertEqual(0, position.opened_timestamp_ns)
        self.assertEqual(Decimal("1.000005"), position.avg_px_open)
        self.assertEqual(3, position.event_count)
        self.assertEqual(
            [order1.cl_ord_id, order2.cl_ord_id, order3.cl_ord_id], position.cl_ord_ids
        )
        self.assertEqual(0, position.closed_timestamp_ns)
        self.assertEqual(Decimal("1.0001"), position.avg_px_close)
        self.assertFalse(position.is_long)
        self.assertFalse(position.is_short)
        self.assertFalse(position.is_open)
        self.assertTrue(position.is_closed)
        self.assertEqual(Money(11.00, USD), position.realized_pnl)
        self.assertEqual(Money(0, USD), position.unrealized_pnl(last))
        self.assertEqual(Money(11.00, USD), position.total_pnl(last))
        self.assertEqual([Money(8.00, USD)], position.commissions())
        self.assertEqual(Money(8.00, USD), position.commission)
        self.assertEqual("Position(FLAT AUD/USD.SIM, id=P-123456)", repr(position))

    def test_pnl_calculation_from_trading_technologies_example(self):
        # https://www.tradingtechnologies.com/xtrader-help/fix-adapter-reference/pl-calculation-algorithm/understanding-pl-calculations/  # noqa

        # Arrange
        order1 = self.order_factory.market(
            ETHUSDT_BINANCE.id,
            OrderSide.BUY,
            Quantity(12),
        )

        order2 = self.order_factory.market(
            ETHUSDT_BINANCE.id,
            OrderSide.BUY,
            Quantity(17),
        )

        order3 = self.order_factory.market(
            ETHUSDT_BINANCE.id,
            OrderSide.SELL,
            Quantity(9),
        )

        order4 = self.order_factory.market(
            ETHUSDT_BINANCE.id,
            OrderSide.SELL,
            Quantity(4),
        )

        order5 = self.order_factory.market(
            ETHUSDT_BINANCE.id,
            OrderSide.BUY,
            Quantity(3),
        )

        # Act
        fill1 = TestStubs.event_order_filled(
            order1,
            instrument=BTCUSDT_BINANCE,
            position_id=PositionId("P-19700101-000000-000-001-1"),
            last_px=Price(100),
        )

        position = Position(fill=fill1)

        fill2 = TestStubs.event_order_filled(
            order2,
            instrument=BTCUSDT_BINANCE,
            position_id=PositionId("P-19700101-000000-000-001-1"),
            last_px=Price(99),
        )

        position.apply(fill2)
        self.assertEqual(Quantity(29), position.quantity)
        self.assertEqual(Money("-2.88300000", USDT), position.realized_pnl)
        self.assertEqual(Decimal("99.41379310344827586206896552"), position.avg_px_open)

        fill3 = TestStubs.event_order_filled(
            order3,
            instrument=BTCUSDT_BINANCE,
            position_id=PositionId("P-19700101-000000-000-001-1"),
            strategy_id=StrategyId("S", "001"),
            last_px=Price(101),
        )

        position.apply(fill3)
        self.assertEqual(Quantity(20), position.quantity)
        self.assertEqual(Money("10.48386207", USDT), position.realized_pnl)
        self.assertEqual(Decimal("99.41379310344827586206896552"), position.avg_px_open)

        fill4 = TestStubs.event_order_filled(
            order4,
            instrument=BTCUSDT_BINANCE,
            position_id=PositionId("P-19700101-000000-000-001-1"),
            strategy_id=StrategyId("S", "001"),
            last_px=Price(105),
        )

        position.apply(fill4)
        self.assertEqual(Quantity(16), position.quantity)
        self.assertEqual(Money("32.40868966", USDT), position.realized_pnl)
        self.assertEqual(Decimal("99.41379310344827586206896552"), position.avg_px_open)

        fill5 = TestStubs.event_order_filled(
            order5,
            instrument=BTCUSDT_BINANCE,
            position_id=PositionId("P-19700101-000000-000-001-1"),
            strategy_id=StrategyId("S", "001"),
            last_px=Price(103),
        )

        position.apply(fill5)
        self.assertEqual(Quantity(19), position.quantity)
        self.assertEqual(Money("32.09968966", USDT), position.realized_pnl)
        self.assertEqual(Decimal("99.98003629764065335753176042"), position.avg_px_open)
        self.assertEqual(
            "Position(LONG 19 ETH/USDT.BINANCE, id=P-19700101-000000-000-001-1)",
            repr(position),
        )

    def test_position_realised_pnl_with_interleaved_order_sides(self):
        # Arrange
        order1 = self.order_factory.market(
            BTCUSDT_BINANCE.id,
            OrderSide.BUY,
            Quantity("12.000000"),
        )

        order2 = self.order_factory.market(
            BTCUSDT_BINANCE.id,
            OrderSide.BUY,
            Quantity("17.000000"),
        )

        order3 = self.order_factory.market(
            BTCUSDT_BINANCE.id,
            OrderSide.SELL,
            Quantity("9.000000"),
        )

        order4 = self.order_factory.market(
            BTCUSDT_BINANCE.id,
            OrderSide.BUY,
            Quantity("3.000000"),
        )

        order5 = self.order_factory.market(
            BTCUSDT_BINANCE.id,
            OrderSide.SELL,
            Quantity("4.000000"),
        )

        # Act
        fill1 = TestStubs.event_order_filled(
            order1,
            instrument=BTCUSDT_BINANCE,
            position_id=PositionId("P-19700101-000000-000-001-1"),
            last_px=Price("10000.00"),
        )

        position = Position(fill=fill1)

        fill2 = TestStubs.event_order_filled(
            order2,
            instrument=BTCUSDT_BINANCE,
            position_id=PositionId("P-19700101-000000-000-001-1"),
            last_px=Price("9999.00"),
        )

        position.apply(fill2)
        self.assertEqual(Quantity("29.000000"), position.quantity)
        self.assertEqual(Money("-289.98300000", USDT), position.realized_pnl)
        self.assertEqual(Decimal("9999.413793103448275862068966"), position.avg_px_open)

        fill3 = TestStubs.event_order_filled(
            order3,
            instrument=BTCUSDT_BINANCE,
            position_id=PositionId("P-19700101-000000-000-001-1"),
            strategy_id=StrategyId("S", "001"),
            last_px=Price("10001.00"),
        )

        position.apply(fill3)
        self.assertEqual(Quantity(20), position.quantity)
        self.assertEqual(Money("-365.71613793", USDT), position.realized_pnl)
        self.assertEqual(Decimal("9999.413793103448275862068966"), position.avg_px_open)

        fill4 = TestStubs.event_order_filled(
            order4,
            instrument=BTCUSDT_BINANCE,
            position_id=PositionId("P-19700101-000000-000-001-1"),
            strategy_id=StrategyId("S", "001"),
            last_px=Price("10003.00"),
        )

        position.apply(fill4)
        self.assertEqual(Quantity(23), position.quantity)
        self.assertEqual(Money("-395.72513793", USDT), position.realized_pnl)
        self.assertEqual(Decimal("9999.881559220389805097451274"), position.avg_px_open)

        fill5 = TestStubs.event_order_filled(
            order5,
            instrument=BTCUSDT_BINANCE,
            position_id=PositionId("P-19700101-000000-000-001-1"),
            strategy_id=StrategyId("S", "001"),
            last_px=Price("10005"),
        )

        position.apply(fill5)
        self.assertEqual(Quantity(19), position.quantity)
        self.assertEqual(Money("-415.27137481", USDT), position.realized_pnl)
        self.assertEqual(Decimal("9999.881559220389805097451274"), position.avg_px_open)
        self.assertEqual(
            "Position(LONG 19.000000 BTC/USDT.BINANCE, id=P-19700101-000000-000-001-1)",
            repr(position),
        )

    def test_calculate_pnl_when_given_position_side_flat_returns_zero(self):
        # Arrange
        order = self.order_factory.market(
            BTCUSDT_BINANCE.id,
            OrderSide.BUY,
            Quantity(12),
        )

        fill = TestStubs.event_order_filled(
            order,
            instrument=BTCUSDT_BINANCE,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S", "001"),
            last_px=Price("10500.00"),
        )

        position = Position(fill=fill)

        # Act
        result = position.calculate_pnl(
            Price("10500.00"), Price("10500.00"), Quantity(100000)
        )

        # Assert
        self.assertEqual(Money(0, USDT), result)

    def test_calculate_pnl_for_long_position_win(self):
        # Arrange
        order = self.order_factory.market(
            BTCUSDT_BINANCE.id,
            OrderSide.BUY,
            Quantity(12),
        )

        fill = TestStubs.event_order_filled(
            order,
            instrument=BTCUSDT_BINANCE,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S", "001"),
            last_px=Price("10500.00"),
        )

        position = Position(fill=fill)

        # Act
        pnl = position.calculate_pnl(
            avg_px_open=Price("10500.00"),
            avg_px_close=Price("10510.00"),
            quantity=Quantity(12),
        )

        # Assert
        self.assertEqual(Money("120.00000000", USDT), pnl)
        self.assertEqual(Money("-126.00000000", USDT), position.realized_pnl)
        self.assertEqual(
            Money("120.00000000", USDT), position.unrealized_pnl(Price("10510.00"))
        )
        self.assertEqual(
            Money("-6.00000000", USDT), position.total_pnl(Price("10510.00"))
        )
        self.assertEqual([Money("126.00000000", USDT)], position.commissions())
        self.assertEqual(Money("126.00000000", USDT), position.commission)

    def test_calculate_pnl_for_long_position_loss(self):
        # Arrange
        order = self.order_factory.market(
            BTCUSDT_BINANCE.id,
            OrderSide.BUY,
            Quantity(12),
        )

        fill = TestStubs.event_order_filled(
            order,
            instrument=BTCUSDT_BINANCE,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S", "001"),
            last_px=Price("10500.00"),
        )

        position = Position(fill=fill)

        # Act
        pnl = position.calculate_pnl(
            avg_px_open=Price("10500.00"),
            avg_px_close=Price("10480.50"),
            quantity=Quantity(10),
        )

        # Assert
        self.assertEqual(Money("-195.00000000", USDT), pnl)
        self.assertEqual(Money("-126.00000000", USDT), position.realized_pnl)
        self.assertEqual(
            Money("-234.00000000", USDT), position.unrealized_pnl(Price("10480.50"))
        )
        self.assertEqual(
            Money("-360.00000000", USDT), position.total_pnl(Price("10480.50"))
        )
        self.assertEqual([Money("126.00000000", USDT)], position.commissions())
        self.assertEqual(Money("126.00000000", USDT), position.commission)

    def test_calculate_pnl_for_short_position_winning(self):
        # Arrange
        order = self.order_factory.market(
            BTCUSDT_BINANCE.id,
            OrderSide.SELL,
            Quantity("10.150000"),
        )

        fill = TestStubs.event_order_filled(
            order,
            instrument=BTCUSDT_BINANCE,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S", "001"),
            last_px=Price("10500.00"),
        )

        position = Position(fill=fill)

        # Act
        pnl = position.calculate_pnl(
            Price("10500.00"),
            Price("10390.00"),
            Quantity("10.150000"),
        )

        # Assert
        self.assertEqual(Money("1116.50000000", USDT), pnl)
        self.assertEqual(
            Money("1116.50000000", USDT), position.unrealized_pnl(Price("10390.00"))
        )
        self.assertEqual(Money("-106.57500000", USDT), position.realized_pnl)
        self.assertEqual([Money("106.57500000", USDT)], position.commissions())
        self.assertEqual(Money("106.57500000", USDT), position.commission)
        self.assertEqual(
            Money("105458.50000000", USDT), position.notional_value(Price("10390.00"))
        )

    def test_calculate_pnl_for_short_position_loss(self):
        # Arrange
        order = self.order_factory.market(
            BTCUSDT_BINANCE.id,
            OrderSide.SELL,
            Quantity("10"),
        )

        fill = TestStubs.event_order_filled(
            order,
            instrument=BTCUSDT_BINANCE,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S", "001"),
            last_px=Price("10500.00"),
        )

        position = Position(fill=fill)

        # Act
        pnl = position.calculate_pnl(
            Price("10500.00"),
            Price("10670.50"),
            Quantity("10.000000"),
        )

        # Assert
        self.assertEqual(Money("-1705.00000000", USDT), pnl)
        self.assertEqual(
            Money("-1705.00000000", USDT), position.unrealized_pnl(Price("10670.50"))
        )
        self.assertEqual(Money("-105.00000000", USDT), position.realized_pnl)
        self.assertEqual([Money("105.00000000", USDT)], position.commissions())
        self.assertEqual(Money("105.00000000", USDT), position.commission)
        self.assertEqual(
            Money("106705.00000000", USDT), position.notional_value(Price("10670.50"))
        )

    def test_calculate_pnl_for_inverse1(self):
        # Arrange
        order = self.order_factory.market(
            XBTUSD_BITMEX.id,
            OrderSide.SELL,
            Quantity(100000),
        )

        fill = TestStubs.event_order_filled(
            order,
            instrument=XBTUSD_BITMEX,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S", "001"),
            last_px=Price("10000.00"),
        )

        position = Position(fill=fill)

        # Act
        pnl = position.calculate_pnl(
            Price("10000.00"),
            Price("11000.00"),
            Quantity(100000),
        )

        # Assert
        self.assertEqual(Money(-10000.00, USD), pnl)
        self.assertEqual(
            Money(-10000.00, USD), position.unrealized_pnl(Price("11000.00"))
        )
        self.assertEqual(Money(0.00, USD), position.realized_pnl)
        self.assertEqual(Money(0.00, USD), position.commission)
        self.assertEqual(
            Money(100000.00, USD), position.notional_value(Price("11000.00"))
        )

    def test_calculate_pnl_for_inverse2(self):
        # Arrange
        order = self.order_factory.market(
            ETHUSD_BITMEX.id,
            OrderSide.SELL,
            Quantity(100000),
        )

        fill = TestStubs.event_order_filled(
            order,
            instrument=ETHUSD_BITMEX,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S", "001"),
            last_px=Price("375.95"),
        )

        position = Position(fill=fill)

        # Act
        # Assert
        self.assertEqual(Money(1582.66, USD), position.unrealized_pnl(Price("370.00")))
        self.assertEqual(
            Money(100000.00, USD), position.notional_value(Price("370.00"))
        )

    def test_calculate_unrealized_pnl_for_long(self):
        # Arrange
        order1 = self.order_factory.market(
            BTCUSDT_BINANCE.id,
            OrderSide.BUY,
            Quantity("2.000000"),
        )

        order2 = self.order_factory.market(
            BTCUSDT_BINANCE.id,
            OrderSide.BUY,
            Quantity("2.000000"),
        )

        fill1 = TestStubs.event_order_filled(
            order1,
            instrument=BTCUSDT_BINANCE,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S", "001"),
            last_px=Price("10500.00"),
        )

        fill2 = TestStubs.event_order_filled(
            order2,
            instrument=BTCUSDT_BINANCE,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S", "001"),
            last_px=Price("10500.00"),
        )

        position = Position(fill=fill1)
        position.apply(fill2)

        # Act
        pnl = position.unrealized_pnl(Price("11505.60"))

        # Assert
        self.assertEqual(Money("4022.40000000", USDT), pnl)
        self.assertEqual(Money("-42.00000000", USDT), position.realized_pnl)
        self.assertEqual([Money("42.00000000", USDT)], position.commissions())
        self.assertEqual(Money("42.00000000", USDT), position.commission)

    def test_calculate_unrealized_pnl_for_short(self):
        # Arrange
        order = self.order_factory.market(
            BTCUSDT_BINANCE.id,
            OrderSide.SELL,
            Quantity("5.912000"),
        )

        fill = TestStubs.event_order_filled(
            order,
            instrument=BTCUSDT_BINANCE,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S", "001"),
            last_px=Price("10505.60"),
        )

        position = Position(fill=fill)

        pnl = position.unrealized_pnl(Price("10407.15"))

        # Assert
        self.assertEqual(Money("582.03640000", USDT), pnl)
        self.assertEqual(Money("-62.10910720", USDT), position.realized_pnl)
        self.assertEqual([Money("62.10910720", USDT)], position.commissions())
        self.assertEqual(Money("62.10910720", USDT), position.commission)

    # TODO: Inverse and Quanto settlement
    # def test_calculate_unrealized_pnl_for_long_inverse(self):
    #     # Arrange
    #     order = self.order_factory.market(
    #         XBTUSD_BITMEX.id,
    #         OrderSide.BUY,
    #         Quantity(100000),
    #     )
    #
    #     fill = TestStubs.event_order_filled(
    #         order,
    #         instrument=XBTUSD_BITMEX,
    #         position_id=PositionId("P-123456"),
    #         strategy_id=StrategyId("S", "001"),
    #         last_px=Price("10500.00"),
    #     )
    #
    #     position = Position(fill)
    #
    #     # Act
    #
    #     pnl = position.unrealized_pnl(Price("11505.60"))
    #
    #     # Assert
    #     self.assertEqual(Money(0.83238969, BTC), pnl)
    #     self.assertEqual(Money(-0.00714286, BTC), position.realized_pnl)
    #     self.assertEqual(Money(-0.00714286, BTC), position.commissions)
    #
    # def test_calculate_unrealized_pnl_for_short_inverse(self):
    #     # Arrange
    #     order = self.order_factory.market(
    #         XBTUSD_BITMEX.id,
    #         OrderSide.SELL,
    #         Quantity(1250000),
    #     )
    #
    #     fill = TestStubs.event_order_filled(
    #         order,
    #         instrument=XBTUSD_BITMEX,
    #         position_id=PositionId("P-123456"),
    #         strategy_id=StrategyId("S", "001"),
    #         last_px=Price("15500.00"),
    #     )
    #
    #     position = Position(fill)
    #
    #     # Act
    #
    #     pnl = position.unrealized_pnl(Price("12506.65"))
    #
    #     # Assert
    #     self.assertEqual(Money(19.30166700, BTC), pnl)
    #     self.assertEqual(Money(-0.06048387, BTC), position.realized_pnl)
    #     self.assertEqual(Money(-0.06048387, BTC), position.commissions)

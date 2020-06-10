# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU General Public License Version 3.0 (the "License");
#  you may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/gpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------

import unittest

from nautilus_trader.model.enums import OrderSide, Currency
from nautilus_trader.model.identifiers import IdTag, PositionId
from nautilus_trader.model.objects import Quantity, Price, Money
from nautilus_trader.model.position import Position
from nautilus_trader.common.guid import TestGuidFactory
from nautilus_trader.common.logging import TestLogger
from nautilus_trader.common.portfolio import Portfolio
from nautilus_trader.common.factories import OrderFactory
from nautilus_trader.common.clock import TestClock

from tests.test_kit.stubs import TestStubs

AUDUSD_FXCM = TestStubs.symbol_audusd_fxcm()
GBPUSD_FXCM = TestStubs.symbol_gbpusd_fxcm()


class PortfolioTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        self.clock = TestClock()
        guid_factor = TestGuidFactory()
        logger = TestLogger()
        self.order_factory = OrderFactory(
            id_tag_trader=IdTag('001'),
            id_tag_strategy=IdTag('001'),
            clock=TestClock())
        self.portfolio = Portfolio(Currency.USD, self.clock, guid_factor, logger)

    def test_initialization(self):
        # Arrange
        # Act
        # Assert
        self.assertEqual(self.clock.date_now(), self.portfolio.date_now)
        self.assertEqual(Money(0, Currency.USD), self.portfolio.daily_pnl_realized)
        self.assertEqual(Money(0, Currency.USD), self.portfolio.total_pnl_realized)
        self.assertEqual(set(), self.portfolio.symbols_open())
        self.assertEqual(set(), self.portfolio.symbols_closed())
        self.assertEqual(set(), self.portfolio.symbols_all())
        self.assertEqual({}, self.portfolio.positions_open())
        self.assertEqual({}, self.portfolio.positions_closed())
        self.assertEqual({}, self.portfolio.positions_all())

    def test_opening_one_position_updates_portfolio(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))
        order_filled = TestStubs.event_order_filled(order, Price(1.00000, 5))
        position = Position(PositionId('P-123456'), order_filled)
        position_opened = TestStubs.event_position_opened(position)

        # Act
        self.portfolio.update(position_opened)

        # Assert
        self.assertEqual({AUDUSD_FXCM}, self.portfolio.symbols_open())
        self.assertEqual(set(), self.portfolio.symbols_closed())
        self.assertEqual({AUDUSD_FXCM}, self.portfolio.symbols_all())
        self.assertEqual({position.id: position}, self.portfolio.positions_open())
        self.assertEqual({}, self.portfolio.positions_closed())
        self.assertEqual({position.id: position}, self.portfolio.positions_all())
        self.assertEqual(Money(0, Currency.USD), self.portfolio.daily_pnl_realized)
        self.assertEqual(Money(0, Currency.USD), self.portfolio.total_pnl_realized)

    def test_can_reset_portfolio(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))
        order_filled = TestStubs.event_order_filled(order, Price(1.00000, 5))
        position = Position(PositionId('P-123456'), order_filled)
        position_opened = TestStubs.event_position_opened(position)

        self.portfolio.update(position_opened)

        # Act
        self.portfolio.reset()

        # Assert
        self.assertEqual(self.clock.date_now(), self.portfolio.date_now)
        self.assertEqual(Money(0, Currency.USD), self.portfolio.daily_pnl_realized)
        self.assertEqual(Money(0, Currency.USD), self.portfolio.total_pnl_realized)
        self.assertEqual(set(), self.portfolio.symbols_open())
        self.assertEqual(set(), self.portfolio.symbols_closed())
        self.assertEqual(set(), self.portfolio.symbols_all())
        self.assertEqual({}, self.portfolio.positions_open())
        self.assertEqual({}, self.portfolio.positions_closed())
        self.assertEqual({}, self.portfolio.positions_all())

    def test_opening_several_positions_updates_portfolio(self):
        # Arrange
        order1 = self.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))
        order2 = self.order_factory.market(
            GBPUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))
        order1_filled = TestStubs.event_order_filled(order1, Price(1.00000, 5))
        order2_filled = TestStubs.event_order_filled(order2, Price(1.00000, 5))

        position1 = Position(PositionId('P-1'), order1_filled)
        position2 = Position(PositionId('P-2'), order2_filled)
        position_opened1 = TestStubs.event_position_opened(position1)
        position_opened2 = TestStubs.event_position_opened(position2)

        # Act
        self.portfolio.update(position_opened1)
        self.portfolio.update(position_opened2)

        # Assert
        self.assertEqual({AUDUSD_FXCM, GBPUSD_FXCM}, self.portfolio.symbols_open())
        self.assertEqual(set(), self.portfolio.symbols_closed())
        self.assertEqual({AUDUSD_FXCM, GBPUSD_FXCM}, self.portfolio.symbols_all())
        self.assertEqual({position1.id: position1, position2.id: position2}, self.portfolio.positions_open())
        self.assertEqual({}, self.portfolio.positions_closed())
        self.assertEqual({position1.id: position1, position2.id: position2}, self.portfolio.positions_all())
        self.assertEqual(Money(0, Currency.USD), self.portfolio.daily_pnl_realized)
        self.assertEqual(Money(0, Currency.USD), self.portfolio.total_pnl_realized)

    def test_modifying_position_updates_portfolio(self):
        # Arrange
        order1 = self.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))
        order1_filled = TestStubs.event_order_filled(order1, Price(1.00000, 5))
        position = Position(PositionId('P-123456'), order1_filled)
        position_opened = TestStubs.event_position_opened(position)

        order2 = self.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.SELL,
            Quantity(50000))
        order2_filled = TestStubs.event_order_filled(order2, Price(1.00000, 5))
        position.apply(order2_filled)
        position_modified = TestStubs.event_position_modified(position)

        # Act
        self.portfolio.update(position_opened)
        self.portfolio.update(position_modified)

        # Assert
        self.assertEqual({AUDUSD_FXCM}, self.portfolio.symbols_open())
        self.assertEqual(set(), self.portfolio.symbols_closed())
        self.assertEqual({AUDUSD_FXCM}, self.portfolio.symbols_all())
        self.assertEqual({position.id: position}, self.portfolio.positions_open())
        self.assertEqual({}, self.portfolio.positions_closed())
        self.assertEqual({position.id: position}, self.portfolio.positions_all())
        self.assertEqual(Money(0, Currency.USD), self.portfolio.daily_pnl_realized)
        self.assertEqual(Money(0, Currency.USD), self.portfolio.total_pnl_realized)

    def test_closing_position_updates_portfolio(self):
        # Arrange
        order1 = self.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))
        order1_filled = TestStubs.event_order_filled(order1, Price(1.00000, 5))
        position = Position(PositionId('P-123456'), order1_filled)
        position_opened = TestStubs.event_position_opened(position)

        order2 = self.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.SELL,
            Quantity(100000))
        order2_filled = TestStubs.event_order_filled(order2, Price(1.00010, 5))
        position.apply(order2_filled)
        position_closed = TestStubs.event_position_closed(position)

        # Act
        self.portfolio.update(position_opened)
        self.portfolio.update(position_closed)

        # Assert
        self.assertEqual(set(), self.portfolio.symbols_open())
        self.assertEqual({AUDUSD_FXCM}, self.portfolio.symbols_closed())
        self.assertEqual({AUDUSD_FXCM}, self.portfolio.symbols_all())
        self.assertEqual({}, self.portfolio.positions_open())
        self.assertEqual({position.id: position}, self.portfolio.positions_closed())
        self.assertEqual({position.id: position}, self.portfolio.positions_all())
        self.assertEqual(Money(10.00, Currency.USD), self.portfolio.daily_pnl_realized)
        self.assertEqual(Money(10.00, Currency.USD), self.portfolio.total_pnl_realized)

    def test_several_positions_with_different_symbols_updates_portfolio(self):
        # Arrange
        order1 = self.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))
        order2 = self.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))
        order3 = self.order_factory.market(
            GBPUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))
        order4 = self.order_factory.market(
            GBPUSD_FXCM,
            OrderSide.SELL,
            Quantity(100000))
        order1_filled = TestStubs.event_order_filled(order1, Price(1.00000, 5))
        order2_filled = TestStubs.event_order_filled(order2, Price(1.00000, 5))
        order3_filled = TestStubs.event_order_filled(order3, Price(1.00000, 5))
        order4_filled = TestStubs.event_order_filled(order4, Price(1.00100, 5))

        position1 = Position(PositionId('P-1'), order1_filled)
        position2 = Position(PositionId('P-2'), order2_filled)
        position3 = Position(PositionId('P-3'), order3_filled)
        position_opened1 = TestStubs.event_position_opened(position1)
        position_opened2 = TestStubs.event_position_opened(position2)
        position_opened3 = TestStubs.event_position_opened(position3)

        position3.apply(order4_filled)
        position_closed = TestStubs.event_position_closed(position3)

        # Act
        self.portfolio.update(position_opened1)
        self.portfolio.update(position_opened2)
        self.portfolio.update(position_opened3)
        self.portfolio.update(position_closed)

        # Assert
        self.assertEqual({AUDUSD_FXCM}, self.portfolio.symbols_open())
        self.assertEqual({GBPUSD_FXCM}, self.portfolio.symbols_closed())
        self.assertEqual({AUDUSD_FXCM, GBPUSD_FXCM}, self.portfolio.symbols_all())
        self.assertEqual({position1.id: position1, position2.id: position2}, self.portfolio.positions_open())
        self.assertEqual({position3.id: position3}, self.portfolio.positions_closed())
        self.assertEqual({position1.id: position1, position2.id: position2, position3.id: position3}, self.portfolio.positions_all())
        self.assertEqual(Money(100.00, Currency.USD), self.portfolio.daily_pnl_realized)
        self.assertEqual(Money(100.00, Currency.USD), self.portfolio.total_pnl_realized)

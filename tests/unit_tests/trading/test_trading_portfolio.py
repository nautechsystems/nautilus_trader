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

from nautilus_trader.backtest.logging import TestLogger
from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.factories import OrderFactory
from nautilus_trader.common.uuid import TestUUIDFactory
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.identifiers import IdTag
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.position import Position
from nautilus_trader.trading.account import Account
from nautilus_trader.trading.portfolio import Portfolio
from tests.test_kit.stubs import TestStubs

AUDUSD_FXCM = TestStubs.symbol_audusd_fxcm()
GBPUSD_FXCM = TestStubs.symbol_gbpusd_fxcm()


class PortfolioTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        self.clock = TestClock()
        uuid_factor = TestUUIDFactory()
        logger = TestLogger(self.clock)
        self.order_factory = OrderFactory(
            strategy_id=StrategyId("S", "001"),
            id_tag_trader=IdTag("001"),
            id_tag_strategy=IdTag("001"),
            clock=TestClock())

        self.account = Account(TestStubs.event_account_state())
        self.portfolio = Portfolio(self.clock, uuid_factor, logger)
        self.portfolio.register_account(self.account)

    def test_initialization(self):
        # Arrange
        # Act
        # Assert
        self.assertEqual(self.clock.utc_now().date(), self.portfolio.date_now)
        # TODO: Implement Portfolio logic

    def test_opening_one_position_updates_portfolio(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))
        order_filled = TestStubs.event_order_filled(order, PositionId("P-123456"), StrategyId("S", "1"), Price("1.00000"))
        position = Position(order_filled)
        position_opened = TestStubs.event_position_opened(position)

        # Act
        self.portfolio.handle_event(position_opened)

        # Assert
        # TODO: Implement Portfolio logic

    def test_reset_portfolio(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))
        order_filled = TestStubs.event_order_filled(order, PositionId("P-123456"), StrategyId("S", "1"), Price("1.00000"))
        position = Position(order_filled)
        position_opened = TestStubs.event_position_opened(position)

        self.portfolio.handle_event(position_opened)

        # Act
        self.portfolio.reset()

        # Assert
        # TODO: Implement Portfolio logic

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
        order1_filled = TestStubs.event_order_filled(order1, PositionId("P-1"), StrategyId("S", "1"), Price("1.00000"))
        order2_filled = TestStubs.event_order_filled(order2, PositionId("P-2"), StrategyId("S", "1"), Price("1.00000"))

        position1 = Position(order1_filled)
        position2 = Position(order2_filled)
        position_opened1 = TestStubs.event_position_opened(position1)
        position_opened2 = TestStubs.event_position_opened(position2)

        # Act
        self.portfolio.handle_event(position_opened1)
        self.portfolio.handle_event(position_opened2)

        # Assert
        # TODO: Implement Portfolio logic

    def test_modifying_position_updates_portfolio(self):
        # Arrange
        order1 = self.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))
        order1_filled = TestStubs.event_order_filled(order1, PositionId("P-123456"), StrategyId("S", "1"), Price("1.00000"))
        position = Position(order1_filled)
        position_opened = TestStubs.event_position_opened(position)

        order2 = self.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.SELL,
            Quantity(50000))
        order2_filled = TestStubs.event_order_filled(order2, PositionId("P-123456"), StrategyId("S", "1"), Price("1.00000"))
        position.apply(order2_filled)
        position_modified = TestStubs.event_position_modified(position)

        # Act
        self.portfolio.handle_event(position_opened)
        self.portfolio.handle_event(position_modified)

        # Assert
        # TODO: Implement Portfolio logic

    def test_closing_position_updates_portfolio(self):
        # Arrange
        order1 = self.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))
        order1_filled = TestStubs.event_order_filled(order1, PositionId("P-123456"), StrategyId("S", "1"), Price("1.00000"))
        position = Position(order1_filled)
        position_opened = TestStubs.event_position_opened(position)

        order2 = self.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.SELL,
            Quantity(100000))
        order2_filled = TestStubs.event_order_filled(order2, PositionId("P-123456"), StrategyId("S", "1"), Price("1.00010"))
        position.apply(order2_filled)
        position_closed = TestStubs.event_position_closed(position)

        # Act
        self.portfolio.handle_event(position_opened)
        self.portfolio.handle_event(position_closed)

        # Assert
        # TODO: Implement Portfolio logic

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
        order1_filled = TestStubs.event_order_filled(order1, PositionId("P-1"), StrategyId("S", "1"), Price("1.00000"))
        order2_filled = TestStubs.event_order_filled(order2, PositionId("P-2"), StrategyId("S", "1"), Price("1.00000"))
        order3_filled = TestStubs.event_order_filled(order3, PositionId("P-3"), StrategyId("S", "1"), Price("1.00000"))
        order4_filled = TestStubs.event_order_filled(order4, PositionId("P-3"), StrategyId("S", "1"), Price("1.00100"))

        position1 = Position(order1_filled)
        position2 = Position(order2_filled)
        position3 = Position(order3_filled)
        position_opened1 = TestStubs.event_position_opened(position1)
        position_opened2 = TestStubs.event_position_opened(position2)
        position_opened3 = TestStubs.event_position_opened(position3)

        position3.apply(order4_filled)
        position_closed = TestStubs.event_position_closed(position3)

        # Act
        self.portfolio.handle_event(position_opened1)
        self.portfolio.handle_event(position_opened2)
        self.portfolio.handle_event(position_opened3)
        self.portfolio.handle_event(position_closed)

        # Assert
        # TODO: Implement Portfolio logic

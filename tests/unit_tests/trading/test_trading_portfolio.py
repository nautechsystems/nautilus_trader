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
from nautilus_trader.core.uuid import uuid4
from nautilus_trader.model.currencies import BTC
from nautilus_trader.model.events import AccountState
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import IdTag
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Money
from nautilus_trader.trading.account import Account
from nautilus_trader.trading.portfolio import Portfolio
from tests.test_kit.stubs import UNIX_EPOCH

FXCM = Venue("FXCM")
BITMEX = Venue("BITMEX")
XBTUSD_BITMEX = Symbol("XBT/USD", BITMEX)


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
            clock=TestClock(),
        )

        state = AccountState(
            AccountId.from_string("BITMEX-1513111-SIMULATED"),
            BTC,
            Money(10., BTC),
            Money(0., BTC),
            Money(0., BTC),
            uuid4(),
            UNIX_EPOCH
        )

        self.account = Account(state)
        self.portfolio = Portfolio(self.clock, uuid_factor, logger)
        self.portfolio.register_account(self.account)

    def test_account_when_no_account_returns_none(self):
        # Arrange
        # Act
        # Assert
        self.assertIsNone(self.portfolio.account(FXCM))

    def test_account_when_account_returns_read_only_facade(self):
        # Arrange
        # Act
        result = self.portfolio.account(BITMEX)

        # Assert
        self.assertEqual(self.account, result)

    def test_unrealized_pnl_when_no_account_returns_none(self):
        # Arrange
        # Act
        # Assert
        self.assertIsNone(self.portfolio.unrealized_pnl(FXCM))

    def test_order_margin_when_no_account_returns_none(self):
        # Arrange
        # Act
        # Assert
        self.assertIsNone(self.portfolio.order_margin(FXCM))

    def test_position_margin_when_no_account_returns_none(self):
        # Arrange
        # Act
        # Assert
        self.assertIsNone(self.portfolio.position_margin(FXCM))

    def test_open_value_when_no_account_returns_none(self):
        # Arrange
        # Act
        # Assert
        self.assertIsNone(self.portfolio.open_value(FXCM))

    # def test_opening_one_position_updates_portfolio(self):
    #     # Arrange
    #     order = self.order_factory.market(
    #         XBTUSD_BITMEX,
    #         OrderSide.BUY,
    #         Quantity("100"),
    #     )
    #
    #     fill = TestStubs.event_order_filled(
    #         order=order,
    #         position_id=PositionId("P-123456"),
    #         strategy_id=StrategyId("S", "1"),
    #         fill_price=Price("1.00000"),
    #     )
    #
    #     position = Position(fill)
    #     position_opened = TestStubs.event_position_opened(position)
    #
    #     # Act
    #     self.portfolio.update_position(position_opened)
    #
    #     # Assert
    #     self.assertEqual(Money(0, BTC), self.portfolio.open_value(BITMEX))

    # def test_reset_portfolio(self):
    #     # Arrange
    #     order = self.order_factory.market(
    #         AUDUSD_FXCM,
    #         OrderSide.BUY,
    #         Quantity(100000))
    #     order_filled = TestStubs.event_order_filled(order, PositionId("P-123456"), StrategyId("S", "1"), Price("1.00000"))
    #     position = Position(order_filled)
    #     position_opened = TestStubs.event_position_opened(position)
    #
    #     self.portfolio.update_position(position_opened)
    #
    #     # Act
    #     self.portfolio.reset()
    #
    #     # Assert

    # def test_opening_several_positions_updates_portfolio(self):
    #     # Arrange
    #     order1 = self.order_factory.market(
    #         AUDUSD_FXCM,
    #         OrderSide.BUY,
    #         Quantity(100000))
    #     order2 = self.order_factory.market(
    #         GBPUSD_FXCM,
    #         OrderSide.BUY,
    #         Quantity(100000))
    #     order1_filled = TestStubs.event_order_filled(order1, PositionId("P-1"), StrategyId("S", "1"), Price("1.00000"))
    #     order2_filled = TestStubs.event_order_filled(order2, PositionId("P-2"), StrategyId("S", "1"), Price("1.00000"))
    #
    #     position1 = Position(order1_filled)
    #     position2 = Position(order2_filled)
    #     position_opened1 = TestStubs.event_position_opened(position1)
    #     position_opened2 = TestStubs.event_position_opened(position2)
    #
    #     # Act
    #     self.portfolio.update_position(position_opened1)
    #     self.portfolio.update_position(position_opened2)
    #
    #     # Assert

    # def test_modifying_position_updates_portfolio(self):
    #     # Arrange
    #     order1 = self.order_factory.market(
    #         AUDUSD_FXCM,
    #         OrderSide.BUY,
    #         Quantity(100000))
    #     order1_filled = TestStubs.event_order_filled(order1, PositionId("P-123456"), StrategyId("S", "1"), Price("1.00000"))
    #     position = Position(order1_filled)
    #     position_opened = TestStubs.event_position_opened(position)
    #
    #     order2 = self.order_factory.market(
    #         AUDUSD_FXCM,
    #         OrderSide.SELL,
    #         Quantity(50000))
    #     order2_filled = TestStubs.event_order_filled(order2, PositionId("P-123456"), StrategyId("S", "1"), Price("1.00000"))
    #     position.apply(order2_filled)
    #     position_modified = TestStubs.event_position_modified(position)
    #
    #     # Act
    #     self.portfolio.update_position(position_opened)
    #     self.portfolio.update_position(position_modified)
    #
    #     # Assert

    # def test_closing_position_updates_portfolio(self):
    #     # Arrange
    #     order1 = self.order_factory.market(
    #         AUDUSD_FXCM,
    #         OrderSide.BUY,
    #         Quantity(100000))
    #     order1_filled = TestStubs.event_order_filled(order1, PositionId("P-123456"), StrategyId("S", "1"), Price("1.00000"))
    #     position = Position(order1_filled)
    #     position_opened = TestStubs.event_position_opened(position)
    #
    #     order2 = self.order_factory.market(
    #         AUDUSD_FXCM,
    #         OrderSide.SELL,
    #         Quantity(100000))
    #     order2_filled = TestStubs.event_order_filled(order2, PositionId("P-123456"), StrategyId("S", "1"), Price("1.00010"))
    #     position.apply(order2_filled)
    #     position_closed = TestStubs.event_position_closed(position)
    #
    #     # Act
    #     self.portfolio.update_position(position_opened)
    #     self.portfolio.update_position(position_closed)
    #
    #     # Assert

    # def test_several_positions_with_different_symbols_updates_portfolio(self):
    #     # Arrange
    #     order1 = self.order_factory.market(
    #         AUDUSD_FXCM,
    #         OrderSide.BUY,
    #         Quantity(100000))
    #     order2 = self.order_factory.market(
    #         AUDUSD_FXCM,
    #         OrderSide.BUY,
    #         Quantity(100000))
    #     order3 = self.order_factory.market(
    #         GBPUSD_FXCM,
    #         OrderSide.BUY,
    #         Quantity(100000))
    #     order4 = self.order_factory.market(
    #         GBPUSD_FXCM,
    #         OrderSide.SELL,
    #         Quantity(100000))
    #     order1_filled = TestStubs.event_order_filled(order1, PositionId("P-1"), StrategyId("S", "1"), Price("1.00000"))
    #     order2_filled = TestStubs.event_order_filled(order2, PositionId("P-2"), StrategyId("S", "1"), Price("1.00000"))
    #     order3_filled = TestStubs.event_order_filled(order3, PositionId("P-3"), StrategyId("S", "1"), Price("1.00000"))
    #     order4_filled = TestStubs.event_order_filled(order4, PositionId("P-3"), StrategyId("S", "1"), Price("1.00100"))
    #
    #     position1 = Position(order1_filled)
    #     position2 = Position(order2_filled)
    #     position3 = Position(order3_filled)
    #     position_opened1 = TestStubs.event_position_opened(position1)
    #     position_opened2 = TestStubs.event_position_opened(position2)
    #     position_opened3 = TestStubs.event_position_opened(position3)
    #
    #     position3.apply(order4_filled)
    #     position_closed = TestStubs.event_position_closed(position3)
    #
    #     # Act
    #     self.portfolio.update_position(position_opened1)
    #     self.portfolio.update_position(position_opened2)
    #     self.portfolio.update_position(position_opened3)
    #     self.portfolio.update_position(position_closed)
    #
    #     # Assert

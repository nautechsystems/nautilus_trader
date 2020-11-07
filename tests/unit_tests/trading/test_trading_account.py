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

from datetime import timedelta
import unittest

from nautilus_trader.backtest.logging import TestLogger
from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.factories import OrderFactory
from nautilus_trader.common.uuid import UUIDFactory
from nautilus_trader.core.uuid import uuid4
from nautilus_trader.data.cache import DataCache
from nautilus_trader.model.currencies import BTC
from nautilus_trader.model.events import AccountState
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.objects import Money
from nautilus_trader.trading.account import Account
from nautilus_trader.trading.portfolio import Portfolio
from tests.test_kit.stubs import UNIX_EPOCH


class AccountTests(unittest.TestCase):

    def setUp(self):
        # Fixture setup
        self.clock = TestClock()
        uuid_factor = UUIDFactory()
        logger = TestLogger(self.clock)
        self.order_factory = OrderFactory(
            trader_id=TraderId("TESTER", "000"),
            strategy_id=StrategyId("S", "001"),
            clock=TestClock(),
        )

        state = AccountState(
            AccountId.from_string("BITMEX-1513111-SIMULATED"),
            BTC,
            Money(10., BTC),
            Money(0., BTC),
            Money(0., BTC),
            uuid4(),
            UNIX_EPOCH,
        )

        self.account = Account(state)
        self.portfolio = Portfolio(self.clock, uuid_factor, logger)
        self.portfolio.register_account(self.account)
        self.portfolio.register_cache(DataCache(logger))

    def test_queries_when_no_portfolio_returns_none(self):
        # Arrange
        state = AccountState(
            AccountId.from_string("BITMEX-1513111-SIMULATED"),
            BTC,
            Money(10., BTC),
            Money(0., BTC),
            Money(0., BTC),
            uuid4(),
            UNIX_EPOCH,
        )

        account = Account(state)

        # Act
        result1 = account.unrealized_pnl()
        result2 = account.margin_balance()
        result3 = account.margin_available()

        # Assert
        self.assertIsNone(result1)
        self.assertIsNone(result2)
        self.assertIsNone(result3)

    def test_initialize_account_with_event(self):
        # Arrange
        # Act
        # Assert
        self.assertEqual(AccountId.from_string("BITMEX-1513111-SIMULATED"), self.account.id)
        self.assertEqual(BTC, self.account.currency)
        self.assertEqual(Money(10, BTC), self.account.balance())
        self.assertEqual(UNIX_EPOCH, self.account.last_event.timestamp)

    def test_apply_given_new_state_event_updates_correctly(self):
        # Arrange
        event = AccountState(
            AccountId.from_string("BITMEX-1513111-SIMULATED"),
            BTC,
            Money(9.5, BTC),
            Money(0., BTC),
            Money(0., BTC),
            uuid4(),
            UNIX_EPOCH + timedelta(minutes=1),
        )

        # Act
        self.account.apply(event)

        # Assert
        self.assertEqual(Money(9.5, BTC), self.account.balance())
        self.assertEqual(event, self.account.last_event)
        self.assertEqual(2, self.account.event_count)

    def test_update_order_margin(self):
        # Arrange
        margin = Money(0.001, BTC)

        # Act
        self.account.update_order_margin(margin)

        # Assert
        self.assertEqual(margin, self.account.order_margin())

    def test_update_position_margin(self):
        # Arrange
        margin = Money(0.0005, BTC)

        # Act
        self.account.update_position_margin(margin)

        # Assert
        self.assertEqual(margin, self.account.position_margin())

    def test_unrealized_pnl_when_no_open_positions_returns_zero(self):
        # Arrange
        # Act
        result = self.account.unrealized_pnl()

        # Assert
        self.assertEqual(result, Money(0, BTC))

    def test_margin_balance_when_no_open_positions_returns_balance(self):
        # Arrange
        # Act
        result = self.account.margin_balance()

        # Assert
        self.assertEqual(result, Money(10., BTC))

    def test_margin_available_when_no_open_positions_returns_balance(self):
        # Arrange
        # Act
        result = self.account.margin_available()

        # Assert
        self.assertEqual(result, Money(10., BTC))

    def test_margin_available_when_open_positions_and_working_orders_returns_expected(self):
        # Arrange
        order_margin = Money(0.2, BTC)
        position_margin = Money(0.15, BTC)

        self.account.update_order_margin(order_margin)
        self.account.update_position_margin(position_margin)

        # Act
        # Assert
        self.assertEqual(Money(10., BTC), self.account.balance())
        self.assertEqual(Money(0.2, BTC), self.account.order_margin())
        self.assertEqual(Money(0.15, BTC), self.account.position_margin())
        self.assertEqual(Money(9.65, BTC), self.account.margin_available())

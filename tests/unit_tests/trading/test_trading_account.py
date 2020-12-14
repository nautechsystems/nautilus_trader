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

from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.factories import OrderFactory
from nautilus_trader.common.logging import TestLogger
from nautilus_trader.core.uuid import uuid4
from nautilus_trader.data.cache import DataCache
from nautilus_trader.model.currencies import BTC
from nautilus_trader.model.currencies import ETH
from nautilus_trader.model.currencies import USD
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
        # Fixture Setup
        self.clock = TestClock()
        logger = TestLogger(self.clock)
        self.order_factory = OrderFactory(
            trader_id=TraderId("TESTER", "000"),
            strategy_id=StrategyId("S", "001"),
            clock=TestClock(),
        )

        self.portfolio = Portfolio(self.clock, logger)
        self.portfolio.register_cache(DataCache(logger))

    def test_instantiate_single_asset_account(self):
        # Arrange
        event = AccountState(
            AccountId("SIM", "001"),
            [Money(1_000_000, USD)],
            [Money(1_000_000, USD)],
            [Money(0, USD)],
            info={"default_currency": "USD"},  # Set the default currency
            event_id=uuid4(),
            event_timestamp=UNIX_EPOCH,
        )

        # Act
        account = Account(event)

        # Wire up account to portfolio
        account.register_portfolio(self.portfolio)
        self.portfolio.register_account(account)

        # Assert
        self.assertEqual(AccountId("SIM", "001"), account.id)
        self.assertEqual(USD, account.default_currency)
        self.assertEqual(event, account.last_event)
        self.assertEqual([event], account.events)
        self.assertEqual(1, account.event_count)
        self.assertEqual(Money(1_000_000, USD), account.balance())
        self.assertEqual(Money(1_000_000, USD), account.balance_free())
        self.assertEqual(Money(0, USD), account.balance_locked())
        self.assertEqual({USD: Money(1_000_000, USD)}, account.balances())
        self.assertEqual({USD: Money(1_000_000, USD)}, account.balances_free())
        self.assertEqual({USD: Money(0, USD)}, account.balances_locked())
        self.assertEqual(Money(0, USD), account.unrealized_pnl())
        self.assertEqual(Money(1_000_000, USD), account.equity())
        self.assertEqual({}, account.init_margins())
        self.assertEqual({}, account.maint_margins())
        self.assertEqual(None, account.init_margin())
        self.assertEqual(None, account.maint_margin())

    def test_instantiate_multi_asset_account(self):
        # Arrange
        event = AccountState(
            AccountId("SIM", "001"),
            [Money("10.00000000", BTC), Money("20.00000000", ETH)],
            [Money("10.00000000", BTC), Money("20.00000000", ETH)],
            [Money("0.00000000", BTC), Money("0.00000000", ETH)],
            info={},  # No default currency set
            event_id=uuid4(),
            event_timestamp=UNIX_EPOCH,
        )

        # Act
        account = Account(event)

        # Wire up account to portfolio
        account.register_portfolio(self.portfolio)
        self.portfolio.register_account(account)

        # Assert
        self.assertEqual(AccountId("SIM", "001"), account.id)
        self.assertEqual(None, account.default_currency)
        self.assertEqual(event, account.last_event)
        self.assertEqual([event], account.events)
        self.assertEqual(1, account.event_count)
        self.assertEqual(Money("10.00000000", BTC), account.balance(BTC))
        self.assertEqual(Money("20.00000000", ETH), account.balance(ETH))
        self.assertEqual(Money("10.00000000", BTC), account.balance_free(BTC))
        self.assertEqual(Money("20.00000000", ETH), account.balance_free(ETH))
        self.assertEqual(Money("0.00000000", BTC), account.balance_locked(BTC))
        self.assertEqual(Money("0.00000000", ETH), account.balance_locked(ETH))
        self.assertEqual({BTC: Money("10.00000000", BTC), ETH: Money("20.00000000", ETH)}, account.balances())
        self.assertEqual({BTC: Money("10.00000000", BTC), ETH: Money("20.00000000", ETH)}, account.balances_free())
        self.assertEqual({BTC: Money("0.00000000", BTC), ETH: Money("0.00000000", ETH)}, account.balances_locked())
        self.assertEqual(Money("0.00000000", BTC), account.unrealized_pnl(BTC))
        self.assertEqual(Money("0.00000000", ETH), account.unrealized_pnl(ETH))
        self.assertEqual(Money("10.00000000", BTC), account.equity(BTC))
        self.assertEqual(Money("20.00000000", ETH), account.equity(ETH))
        self.assertEqual({}, account.init_margins())
        self.assertEqual({}, account.maint_margins())
        self.assertEqual(None, account.init_margin(BTC))
        self.assertEqual(None, account.init_margin(ETH))
        self.assertEqual(None, account.maint_margin(BTC))
        self.assertEqual(None, account.maint_margin(ETH))

    def test_apply_given_new_state_event_updates_correctly(self):
        # Arrange
        event1 = AccountState(
            AccountId("SIM", "001"),
            [Money("10.00000000", BTC), Money("20.00000000", ETH)],
            [Money("10.00000000", BTC), Money("20.00000000", ETH)],
            [Money("0.00000000", BTC), Money("0.00000000", ETH)],
            info={},  # No default currency set
            event_id=uuid4(),
            event_timestamp=UNIX_EPOCH,
        )

        # Act
        account = Account(event1)

        # Wire up account to portfolio
        account.register_portfolio(self.portfolio)
        self.portfolio.register_account(account)

        event2 = AccountState(
            AccountId("SIM", "001"),
            [Money("9.00000000", BTC), Money("20.00000000", ETH)],
            [Money("8.50000000", BTC), Money("20.00000000", ETH)],
            [Money("0.50000000", BTC), Money("0.00000000", ETH)],
            info={},  # No default currency set
            event_id=uuid4(),
            event_timestamp=UNIX_EPOCH,
        )

        # Act
        account.apply(event2)

        # Assert
        self.assertEqual(event2, account.last_event)
        self.assertEqual([event1, event2], account.events)
        self.assertEqual(2, account.event_count)
        self.assertEqual(Money("9.00000000", BTC), account.balance(BTC))
        self.assertEqual(Money("8.50000000", BTC), account.balance_free(BTC))
        self.assertEqual(Money("0.50000000", BTC), account.balance_locked(BTC))
        self.assertEqual(Money("20.00000000", ETH), account.balance(ETH))
        self.assertEqual(Money("20.00000000", ETH), account.balance_free(ETH))
        self.assertEqual(Money("0.00000000", ETH), account.balance_locked(ETH))

    def test_update_init_margin(self):
        # Arrange
        event = AccountState(
            AccountId("SIM", "001"),
            [Money("10.00000000", BTC), Money("20.00000000", ETH)],
            [Money("10.00000000", BTC), Money("20.00000000", ETH)],
            [Money("0.00000000", BTC), Money("0.00000000", ETH)],
            info={},  # No default currency set
            event_id=uuid4(),
            event_timestamp=UNIX_EPOCH,
        )

        # Act
        account = Account(event)

        # Wire up account to portfolio
        account.register_portfolio(self.portfolio)
        self.portfolio.register_account(account)

        margin = Money("0.00100000", BTC)

        # Act
        account.update_init_margin(margin)

        # Assert
        self.assertEqual(margin, account.init_margin(BTC))
        self.assertEqual({BTC: margin}, account.init_margins())

    def test_update_maint_margin(self):
        # Arrange
        event = AccountState(
            AccountId("SIM", "001"),
            [Money("10.00000000", BTC), Money("20.00000000", ETH)],
            [Money("10.00000000", BTC), Money("20.00000000", ETH)],
            [Money("0.00000000", BTC), Money("0.00000000", ETH)],
            info={},  # No default currency set
            event_id=uuid4(),
            event_timestamp=UNIX_EPOCH,
        )

        # Act
        account = Account(event)

        # Wire up account to portfolio
        account.register_portfolio(self.portfolio)
        self.portfolio.register_account(account)

        margin = Money("0.00050000", BTC)

        # Act
        account.update_maint_margin(margin)

        # Assert
        self.assertEqual(margin, account.maint_margin(BTC))
        self.assertEqual({BTC: margin}, account.maint_margins())

    def test_unrealized_pnl_with_single_asset_account_when_no_open_positions_returns_zero(self):
        # Arrange
        event = AccountState(
            AccountId("SIM", "001"),
            balances=[Money(1_000_000, USD)],
            balances_free=[Money(1_000_000, USD)],
            balances_locked=[Money(0, USD)],
            info={"default_currency": "USD"},  # No default currency set
            event_id=uuid4(),
            event_timestamp=UNIX_EPOCH,
        )

        account = Account(event)

        # Wire up account to portfolio
        account.register_portfolio(self.portfolio)
        self.portfolio.register_account(account)

        # Act
        result = account.unrealized_pnl()

        # Assert
        self.assertEqual(result, Money(0, USD))

    def test_unrealized_pnl_with_multi_asset_account_when_no_open_positions_returns_zero(self):
        # Arrange
        event = AccountState(
            AccountId("SIM", "001"),
            [Money("10.00000000", BTC), Money("20.00000000", ETH)],
            [Money("10.00000000", BTC), Money("20.00000000", ETH)],
            [Money("0.00000000", BTC), Money("0.00000000", ETH)],
            info={},  # No default currency set
            event_id=uuid4(),
            event_timestamp=UNIX_EPOCH,
        )

        account = Account(event)

        # Wire up account to portfolio
        account.register_portfolio(self.portfolio)
        self.portfolio.register_account(account)

        # Act
        result = account.unrealized_pnl(BTC)

        # Assert
        self.assertEqual(result, Money("0.00000000", BTC))

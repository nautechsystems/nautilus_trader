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
from nautilus_trader.common.uuid import TestUUIDFactory
from nautilus_trader.core.uuid import uuid4
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
        uuid_factor = TestUUIDFactory()
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
            UNIX_EPOCH
        )

        self.account = Account(state)
        self.portfolio = Portfolio(self.clock, uuid_factor, logger)
        self.portfolio.register_account(self.account)

    def test_initialize_account_with_event(self):
        # Arrange
        # Act
        # Assert
        self.assertEqual(AccountId.from_string("BITMEX-1513111-SIMULATED"), self.account.id)
        self.assertEqual(BTC, self.account.currency)
        self.assertEqual(Money(10, BTC), self.account.balance())
        self.assertEqual(UNIX_EPOCH, self.account.last_event().timestamp)

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

    def test_apply_given_new_state_event_updates_correctly(self):
        # Arrange
        event = AccountState(
            AccountId.from_string("BITMEX-1513111-SIMULATED"),
            BTC,
            Money(9.5, BTC),
            Money(0., BTC),
            Money(0., BTC),
            uuid4(),
            UNIX_EPOCH + timedelta(minutes=1)
        )

        # Act
        self.account.apply(event)

        # Assert
        self.assertEqual(Money(9.5, BTC), self.account.balance())
        self.assertEqual(event, self.account.last_event())
        self.assertEqual(2, self.account.event_count())

    # def test_update_unrealized_pnl_adjusts_account_correctly(self):
    #     # Arrange
    #     # Act
    #     self.account.update_unrealized_pnl(Money(1.5, BTC))
    #
    #     # Assert
    #     self.assertEqual(Money(10, BTC), self.account.balance)
    #     self.assertEqual(Money(11.5, BTC), self.account.margin_balance)
    #     self.assertEqual(Money(0, BTC), self.account.order_margin)
    #     self.assertEqual(Money(0, BTC), self.account.position_margin)
    #     self.assertEqual(Money(11.5, BTC), self.account.margin_available)
    #     self.assertEqual(UNIX_EPOCH, self.account.last_event().timestamp)
    #
    # def test_update_order_margin_adjusts_account_correctly(self):
    #     # Arrange
    #     self.account.update_unrealized_pnl(Money(-0.5, BTC))
    #
    #     # Act
    #     self.account.update_order_margin(Money(0.0015, BTC))
    #
    #     # Assert
    #     self.assertEqual(Money(10, BTC), self.account.balance)
    #     self.assertEqual(Money(9.5, BTC), self.account.margin_balance)
    #     self.assertEqual(Money(0.0015, BTC), self.account.order_margin)
    #     self.assertEqual(Money(0, BTC), self.account.position_margin)
    #     self.assertEqual(Money(9.4985, BTC), self.account.margin_available)
    #     self.assertEqual(UNIX_EPOCH, self.account.last_event().timestamp)
    #
    # def test_update_position_margin_adjusts_account_correctly(self):
    #     # Arrange
    #     self.account.update_unrealized_pnl(Money(-0.8, BTC))
    #     self.account.update_order_margin(Money(0.0015, BTC))
    #
    #     # Act
    #     self.account.update_position_margin(Money(0.02, BTC))
    #
    #     # Assert
    #     self.assertEqual(Money(10, BTC), self.account.balance)
    #     self.assertEqual(Money(9.2, BTC), self.account.margin_balance)
    #     self.assertEqual(Money(0.0015, BTC), self.account.order_margin)
    #     self.assertEqual(Money(0.02, BTC), self.account.position_margin)
    #     self.assertEqual(Money(9.1785, BTC), self.account.margin_available)
    #     self.assertEqual(UNIX_EPOCH, self.account.last_event().timestamp)

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

from nautilus_trader.core.types import ValidString
from nautilus_trader.core.uuid import uuid4
from nautilus_trader.core.decimal import Decimal
from nautilus_trader.model.enums import Currency
from nautilus_trader.model.objects import Money
from nautilus_trader.model.events import AccountStateEvent
from nautilus_trader.model.identifiers import Brokerage, AccountNumber, AccountId
from nautilus_trader.common.account import Account
from tests.test_kit.stubs import UNIX_EPOCH


class AccountTests(unittest.TestCase):

    def test_can_initialize_account_with_event(self):
        # Arrange
        event = AccountStateEvent(
            AccountId.py_from_string("FXCM-123456-SIMULATED"),
            Currency.AUD,
            Money(1000000, Currency.AUD),
            Money(1000000, Currency.AUD),
            Money(0, Currency.AUD),
            Money(0, Currency.AUD),
            Money(0, Currency.AUD),
            Decimal(0),
            ValidString("N"),
            uuid4(),
            UNIX_EPOCH)

        # Act
        account = Account(event)

        # Assert
        self.assertEqual(AccountId.py_from_string("FXCM-123456-SIMULATED"), account.id)
        self.assertEqual(Currency.AUD, account.currency)
        self.assertEqual(Money(1000000, Currency.AUD), account.free_equity)
        self.assertEqual(Money(1000000, Currency.AUD), account.cash_start_day)
        self.assertEqual(Money(0, Currency.AUD), account.cash_activity_day)
        self.assertEqual(Money(0, Currency.AUD), account.margin_used_liquidation)
        self.assertEqual(Money(0, Currency.AUD), account.margin_used_maintenance)
        self.assertEqual(Decimal(0), account.margin_ratio)
        self.assertEqual("N", account.margin_call_status.value)
        self.assertEqual(UNIX_EPOCH, account.last_updated)

    def test_can_calculate_free_equity_when_greater_than_zero(self):
        # Arrange
        event = AccountStateEvent(
            AccountId.py_from_string("FXCM-123456-SIMULATED"),
            Currency.AUD,
            Money(100000, Currency.AUD),
            Money(100000, Currency.AUD),
            Money(0, Currency.AUD),
            Money(1000, Currency.AUD),
            Money(2000, Currency.AUD),
            Decimal(0),
            ValidString("N"),
            uuid4(),
            UNIX_EPOCH)

        # Act
        account = Account(event)

        # Assert
        self.assertEqual(AccountId.py_from_string("FXCM-123456-SIMULATED"), account.id)
        self.assertEqual(Brokerage("FXCM"), account.broker)
        self.assertEqual(AccountNumber("123456"), account.account_number)
        self.assertEqual(Currency.AUD, account.currency)
        self.assertEqual(Money(97000, Currency.AUD), account.free_equity)
        self.assertEqual(Money(100000, Currency.AUD), account.cash_start_day)
        self.assertEqual(Money(0, Currency.AUD), account.cash_activity_day)
        self.assertEqual(Money(1000, Currency.AUD), account.margin_used_liquidation)
        self.assertEqual(Money(2000, Currency.AUD), account.margin_used_maintenance)
        self.assertEqual(Decimal(0), account.margin_ratio)
        self.assertEqual("N", account.margin_call_status.value)
        self.assertEqual(UNIX_EPOCH, account.last_updated)

    def test_can_calculate_free_equity_when_zero(self):
        # Arrange
        event = AccountStateEvent(
            AccountId.py_from_string("FXCM-123456-SIMULATED"),
            Currency.AUD,
            Money(20000, Currency.AUD),
            Money(100000, Currency.AUD),
            Money(0, Currency.AUD),
            Money(0, Currency.AUD),
            Money(20000, Currency.AUD),
            Decimal(0),
            ValidString("N"),
            uuid4(),
            UNIX_EPOCH)

        # Act
        account = Account(event)

        # Assert
        self.assertEqual(AccountId.py_from_string("FXCM-123456-SIMULATED"), account.id)
        self.assertEqual(Brokerage("FXCM"), account.broker)
        self.assertEqual(AccountNumber("123456"), account.account_number)
        self.assertEqual(Currency.AUD, account.currency)
        self.assertEqual(Money(0, Currency.AUD), account.free_equity)
        self.assertEqual(Money(100000, Currency.AUD), account.cash_start_day)
        self.assertEqual(Money(0, Currency.AUD), account.cash_activity_day)
        self.assertEqual(Money(0, Currency.AUD), account.margin_used_liquidation)
        self.assertEqual(Money(20000, Currency.AUD), account.margin_used_maintenance)
        self.assertEqual(Decimal(0), account.margin_ratio)
        self.assertEqual("N", account.margin_call_status.value)
        self.assertEqual(UNIX_EPOCH, account.last_updated)

    def test_can_calculate_free_equity_when_negative(self):
        # Arrange
        event = AccountStateEvent(
            AccountId.py_from_string("FXCM-123456-SIMULATED"),
            Currency.AUD,
            Money(20000, Currency.AUD),
            Money(100000, Currency.AUD),
            Money(0, Currency.AUD),
            Money(10000, Currency.AUD),
            Money(20000, Currency.AUD),
            Decimal(0),
            ValidString("N"),
            uuid4(),
            UNIX_EPOCH)

        # Act
        account = Account(event)

        # Assert
        self.assertEqual(AccountId.py_from_string("FXCM-123456-SIMULATED"), account.id)
        self.assertEqual(Brokerage("FXCM"), account.broker)
        self.assertEqual(AccountNumber("123456"), account.account_number)
        self.assertEqual(Currency.AUD, account.currency)
        self.assertEqual(Money(0, Currency.AUD), account.free_equity)
        self.assertEqual(Money(100000, Currency.AUD), account.cash_start_day)
        self.assertEqual(Money(0, Currency.AUD), account.cash_activity_day)
        self.assertEqual(Money(10000, Currency.AUD), account.margin_used_liquidation)
        self.assertEqual(Money(20000, Currency.AUD), account.margin_used_maintenance)
        self.assertEqual(Decimal(0), account.margin_ratio)
        self.assertEqual("N", account.margin_call_status.value)
        self.assertEqual(UNIX_EPOCH, account.last_updated)

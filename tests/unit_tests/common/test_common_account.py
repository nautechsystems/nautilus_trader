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

from nautilus_trader.common.account import Account
from nautilus_trader.core.uuid import uuid4
from nautilus_trader.model.currency import Currency
from nautilus_trader.model.events import AccountState
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.objects import Money
from tests.test_kit.stubs import UNIX_EPOCH


class AccountTests(unittest.TestCase):

    def test_initialize_account_with_event(self):
        # Arrange
        event = AccountState(
            AccountId.py_from_string("BITMEX-1513111-SIMULATED"),
            Currency.BTC(),
            Money(10., Currency.BTC()),
            Money(10., Currency.BTC()),
            Money(10., Currency.BTC()),
            uuid4(),
            UNIX_EPOCH)

        # Act
        account = Account(event)

        # Assert
        self.assertEqual(AccountId.py_from_string("BITMEX-1513111-SIMULATED"), account.id)
        self.assertEqual(Currency.BTC(), account.currency)
        self.assertEqual(Money(10., Currency.BTC()), account.balance)
        self.assertEqual(Money(10., Currency.BTC()), account.free_equity)
        self.assertEqual(Money(10., Currency.BTC()), account.margin_balance)
        self.assertEqual(Money(10., Currency.BTC()), account.margin_available)
        self.assertEqual(UNIX_EPOCH, account.last_event().timestamp)

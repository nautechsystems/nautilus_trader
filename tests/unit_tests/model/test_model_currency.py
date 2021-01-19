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

import unittest

from parameterized import parameterized

from nautilus_trader.model.currencies import AUD
from nautilus_trader.model.currencies import BTC
from nautilus_trader.model.currencies import ETH
from nautilus_trader.model.currencies import GBP
from nautilus_trader.model.currency import Currency
from nautilus_trader.model.enums import CurrencyType
from tests.test_kit.stubs import TestStubs


AUDUSD_SIM = TestStubs.symbol_audusd()
GBPUSD_SIM = TestStubs.symbol_gbpusd()


class CurrencyTests(unittest.TestCase):

    def test_currency_equality(self):
        # Arrange
        currency1 = Currency("AUD", precision=2, currency_type=CurrencyType.FIAT)
        currency2 = Currency("AUD", precision=2, currency_type=CurrencyType.FIAT)
        currency3 = Currency("GBP", precision=2, currency_type=CurrencyType.FIAT)

        # Act
        # Assert
        self.assertTrue(currency1 == currency1)
        self.assertTrue(currency1 == currency2)
        self.assertTrue(currency1 != currency3)

    def test_currency_hash(self):
        # Arrange
        currency = Currency("AUD", precision=2, currency_type=CurrencyType.FIAT)

        # Act
        # Assert
        self.assertEqual(int, type(hash(currency)))
        self.assertEqual(hash(currency), hash(currency))

    def test_str_repr(self):
        # Arrange
        currency = Currency("AUD", precision=2, currency_type=CurrencyType.FIAT)

        # Act
        # Assert
        self.assertEqual("AUD", str(currency))
        self.assertEqual("Currency(code=AUD, precision=2, type=FIAT)", repr(currency))

    def test_from_str_given_unknown_code_returns_none(self):
        # Arrange
        # Act
        result = Currency.from_str("SOME_CURRENCY")

        # Assert
        self.assertIsNone(result)

    @parameterized.expand([
        ["AUD", AUD],
        ["GBP", GBP],
        ["BTC", BTC],
        ["ETH", ETH],
    ])
    def test_from_str(self, string, expected):
        # Arrange
        # Act
        result = Currency.from_str(string)

        # Assert
        self.assertEqual(expected, result)

    @parameterized.expand([
        ["AUD", True],
        ["ZZZ", False],
    ])
    def test_is_fiat(self, string, expected):
        # Arrange
        # Act
        result = Currency.is_fiat(string)

        # Assert
        self.assertEqual(expected, result)

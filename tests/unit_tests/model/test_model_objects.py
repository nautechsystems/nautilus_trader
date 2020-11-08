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

import decimal
import unittest

from parameterized import parameterized

from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.objects import Price
from nautilus_trader.core.decimal import Decimal
from nautilus_trader.model.currencies import BTC
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.objects import Money


class PriceTests(unittest.TestCase):

    def test_str_repr(self):
        # Arrange
        price = Price(1.00000, 5)

        # Act
        # Assert
        self.assertEqual("1.00000", str(price))
        self.assertEqual("Price('1.00000')", repr(price))


class QuantityTests(unittest.TestCase):

    def test_instantiate_with_negative_value_raises_exception(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(ValueError, Quantity, -1)

    @parameterized.expand([
        [Quantity("0"), "0"],
        [Quantity("10.05"), "10.05"],
        [Quantity("1000"), "1,000"],
        [Quantity("1112"), "1,112"],
        [Quantity("120100"), "120,100"],
        [Quantity("200000"), "200,000"],
        [Quantity("1000000"), "1,000,000"],
        [Quantity("2500000"), "2,500,000"],
        [Quantity("1111111"), "1,111,111"],
        [Quantity("2523000"), "2,523,000"],
        [Quantity("100000000"), "100,000,000"],
    ])
    def test_str_and_to_string(self, value, expected):
        # Arrange
        # Act
        # Assert
        self.assertEqual(expected, Quantity(value).to_string())

    def test_str_repr(self):
        # Arrange
        quantity = Quantity(2100.1666666, 6)

        # Act
        # Assert
        self.assertEqual("2100.166667", str(quantity))
        self.assertEqual("Quantity('2100.166667')", repr(quantity))


class MoneyTests(unittest.TestCase):

    def test_instantiate_with_none_currency_raises_type_error(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(TypeError, Money, 1.0, None)

    @parameterized.expand([
        [0, Money(0, USD)],
        [1, Money("1", USD)],
        [-1, Money("-1", USD)],
        ["0", Money(0, USD)],
        ["0.0", Money(0, USD)],
        ["-0.0", Money(0, USD)],
        ["1.0", Money("1", USD)],
        ["-1.0", Money("-1", USD)],
        [decimal.Decimal(), Money(0, USD)],
        [decimal.Decimal("1.1"), Money("1.1", USD)],
        [decimal.Decimal("-1.1"), Money("-1.1", USD)],
        [Decimal(), Money(0, USD)],
        [Decimal("1.1"), Money("1.1", USD)],
        [Decimal("-1.1"), Money("-1.1", USD)],
    ])
    def test_instantiate_with_various_valid_inputs_returns_expected_money(self, value, expected):
        # Arrange
        # Act
        money = Money(value, USD)

        # Assert
        self.assertEqual(expected, money)

    @parameterized.expand([
        ["0", -0, False],
        ["-0", 0, False],
        ["-1", -1, False],
    ])
    def test_equality_with_different_currencies_returns_false(self, value1, value2, expected):
        # Arrange
        # Act
        result = Money(value1, USD) == Money(value2, BTC)

        # Assert
        self.assertEqual(expected, result)

    @parameterized.expand([
        [0, 0, False, False, False, False],
        [1, 0, False, False, False, False],
        [-1, 0, False, False, False, False],
    ])
    def test_comparisons_with_different_currencies_returns_false(
            self,
            value1,
            value2,
            expected1,
            expected2,
            expected3,
            expected4,
    ):
        # Arrange
        # Act
        result1 = Money(value1, USD) > Money(value2, BTC)
        result2 = Money(value1, USD) >= Money(value2, BTC)
        result3 = Money(value1, USD) <= Money(value2, BTC)
        result4 = Money(value1, USD) < Money(value2, BTC)

        # Assert
        self.assertEqual(expected1, result1)
        self.assertEqual(expected2, result2)
        self.assertEqual(expected3, result3)
        self.assertEqual(expected4, result4)

    def test_from_string_with_no_decimal(self):
        # Arrange
        # Act
        money = Money(1, USD)

        # Assert
        self.assertEqual(1.0, money.as_double())
        self.assertEqual("1.00", str(money))

    def test_initialized_with_many_decimals_rounds_to_currency_precision(self):
        # Arrange
        # Act
        result1 = Money(1000.333, USD)
        result2 = Money(5005.556666, USD)

        # Assert
        self.assertEqual("1,000.33 USD", result1.to_string())
        self.assertEqual("5,005.56 USD", result2.to_string())

    def test_str(self):
        # Arrange
        money0 = Money(0, USD)
        money1 = Money(1, USD)
        money2 = Money(1000000, USD)

        # Act
        # Assert
        self.assertEqual("0.00", str(money0))
        self.assertEqual("1.00", str(money1))
        self.assertEqual("1000000.00", str(money2))
        self.assertEqual("1,000,000.00 USD", money2.to_string())

    def test_repr(self):
        # Arrange
        money = Money("1.00", USD)

        # Act
        result = repr(money)

        # Assert
        self.assertEqual("Money('1.00', USD)", result)

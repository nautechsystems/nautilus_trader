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

from nautilus_trader.core.fraction import Fraction
from nautilus_trader.model.currencies import BTC
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.objects import Money


class MoneyTests(unittest.TestCase):

    def test_instantiate_with_none_value_raises_type_error(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(TypeError, Money, None, USD)

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
        [Fraction(), Money(0, USD)],
        [Fraction("1.1"), Money("1.1", USD)],
        [Fraction("-1.1"), Money("-1.1", USD)],
    ])
    def test_instantiate_with_various_valid_inputs_returns_expected_decimal(self, value, expected):
        # Arrange
        # Act
        money = Money(value, USD)

        # Assert
        self.assertEqual(expected, money)

    @parameterized.expand([
        ["0", 0],
        ["-0", 0],
        ["-1", -1],
        ["1", 1],
        ["1.1", 1.1],
        ["-1.1", -1.1],
    ])
    def test_as_double_with_various_values_returns_expected_value(self, value, expected):
        # Arrange
        # Act
        result = Money(value, USD).as_double()

        # Assert
        self.assertEqual(expected, result)

    @parameterized.expand([
        ["0", -0, True],
        ["-0", 0, True],
        ["-1", -1, True],
        ["1", 1, True],
        ["1.1", "1.1", True],
        ["-1.1", "-1.1", True],
        ["0", 1, False],
        ["-1", 0, False],
        ["-1", -2, False],
        ["1", 2, False],
        ["1.1", "1.12", False],
        ["-1.12", "-1.1", False],
    ])
    def test_equality_with_various_values_returns_expected_result(self, value1, value2, expected):
        # Arrange
        # Act
        result = Money(value1, USD) == Money(value2, USD)

        # Assert
        self.assertEqual(expected, result)

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
        [0, 0, False, True, True, False],
        [1, 0, True, True, False, False],
        [-1, 0, False, False, True, True],
    ])
    def test_comparisons_with_various_values_returns_expected_result(
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
        result1 = Money(value1, USD) > Money(value2, USD)
        result2 = Money(value1, USD) >= Money(value2, USD)
        result3 = Money(value1, USD) <= Money(value2, USD)
        result4 = Money(value1, USD) < Money(value2, USD)

        # Assert
        self.assertEqual(expected1, result1)
        self.assertEqual(expected2, result2)
        self.assertEqual(expected3, result3)
        self.assertEqual(expected4, result4)

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

    @parameterized.expand([
        [Money(0, USD), Money(0, USD), Fraction, 0],
        [Money(0, USD), Money("1.1", USD), Fraction, Fraction("1.1")],
        [Money(0, USD), 0, Fraction, 0],
        [Money(0, USD), 1, Fraction, 1],
        [Money(0, USD), 0.0, float, 0],
        [Money(0, USD), 1.0, float, 1.0],
        [Money("1", USD), Fraction("1.1"), Fraction, Fraction("2.1")],
    ])
    def test_addition_with_various_types_returns_expected_result(
            self,
            value1,
            value2,
            expected_type,
            expected_value):
        # Arrange
        # Act
        result = value1 + value2

        # Assert
        self.assertEqual(expected_type, type(result))
        self.assertEqual(expected_value, result)

    @parameterized.expand([
        [Money(0, USD), Money(0, USD), Fraction, 0],
        [Money(0, USD), Money("1.1", USD), Fraction, Fraction("-1.1")],
        [Money(0, USD), 0, Fraction, 0],
        [Money(0, USD), 1, Fraction, -1],
        [Money(0, USD), 0.0, float, 0],
        [Money(0, USD), 1.0, float, -1.0],
        [Money("1", USD), Fraction("1.1"), Fraction, Fraction("-0.1")],
    ])
    def test_subtraction_with_various_types_returns_expected_result(
            self,
            value1,
            value2,
            expected_type,
            expected_value):
        # Arrange
        # Act
        result = value1 - value2

        # Assert
        self.assertEqual(expected_type, type(result))
        self.assertEqual(expected_value, result)

    @parameterized.expand([
        [Money(0, USD), 0, Fraction, 0],
        [Money(1, USD), 1, Fraction, 1],
        [Money(2, USD), 1.0, float, 2],
        [Money("1.1", USD), Fraction("1.1"), Fraction, Fraction("1.21")],
    ])
    def test_multiplication_with_various_types_returns_expected_result(
            self,
            value1,
            value2,
            expected_type,
            expected_value):
        # Arrange
        # Act
        result = value1 * value2

        # Assert
        self.assertEqual(expected_type, type(result))
        self.assertEqual(expected_value, result)

    @parameterized.expand([
        [Money(0, USD), 1, Fraction, 0],
        [Money(1, USD), 2, Fraction, 0.5],
        [Money(2, USD), 1.0, float, 2],
        [Money("1.1", USD), Fraction("1.2"), Fraction, 0.9166666666666666],
    ])
    def test_division_with_various_types_returns_expected_result(
            self,
            value1,
            value2,
            expected_type,
            expected_value):
        # Arrange
        # Act
        result = value1 / value2

        # Assert
        self.assertEqual(expected_type, type(result))
        self.assertAlmostEqual(expected_value, result)

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
        self.assertEqual("1,000.33 USD", result1.to_string_formatted())
        self.assertEqual("5,005.56 USD", result2.to_string_formatted())

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
        self.assertEqual("1,000,000.00 USD", money2.to_string_formatted())

    def test_repr(self):
        # Arrange
        money = Money("1.00", USD)

        # Act
        result = repr(money)

        # Assert
        self.assertTrue(result.startswith("<Money('1.00', USD) object at"))

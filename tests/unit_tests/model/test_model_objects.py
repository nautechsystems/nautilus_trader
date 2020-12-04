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
from decimal import Decimal
import unittest

from parameterized import parameterized

from nautilus_trader.model.currencies import BTC
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.objects import BaseDecimal
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity


class BaseDecimalTests(unittest.TestCase):

    def test_instantiate_with_none_value_raises_type_error(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(TypeError, BaseDecimal, None)

    def test_instantiate_with_float_value_raises_type_error(self):  # User should use .from_float()
        # Arrange
        # Act
        # Assert
        self.assertRaises(TypeError, BaseDecimal, 1.1)

    def test_instantiate_with_negative_precision_argument_returns_zero_decimal(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(ValueError, BaseDecimal, 1.11, -1)

    def test_rounding_with_bogus_mode_raises_exception(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(TypeError, BaseDecimal, 1.11, 1, "UNKNOWN")

    @parameterized.expand([
        [BaseDecimal("2.15"), -1, Decimal("0E+1")],
        [BaseDecimal("2.15"), 0, Decimal("2")],
        [BaseDecimal("2.15"), 1, Decimal("2.2")],
        [BaseDecimal("2.255"), 2, Decimal("2.26")],
    ])
    def test_round_with_various_digits_returns_expected_decimal(self, value, precision, expected):
        # Arrange
        # Act
        result = round(value, precision)

        # Assert
        self.assertEqual(expected, result)

    @parameterized.expand([
        [BaseDecimal("-0"), Decimal("0")],
        [BaseDecimal("0"), Decimal("0")],
        [BaseDecimal("1"), Decimal("1")],
        [BaseDecimal("-1"), Decimal("1")],
        [BaseDecimal("-1.1"), Decimal("1.1")],
    ])
    def test_abs_with_various_values_returns_expected_decimal(self, value, expected):
        # Arrange
        # Act
        result = abs(value)

        # Assert
        self.assertEqual(expected, result)

    @parameterized.expand([
        [BaseDecimal("-1"), Decimal("-1")],  # Matches built-in decimal.Decimal behaviour
        [BaseDecimal("0"), Decimal("0")],
    ])
    def test_pos_with_various_values_returns_expected_decimal(self, value, expected):
        # Arrange
        # Act
        result = +value

        # Assert
        self.assertEqual(expected, result)

    @parameterized.expand([
        [BaseDecimal("1"), Decimal("-1")],
        [BaseDecimal("0"), Decimal("0")],
    ])
    def test_neg_with_various_values_returns_expected_decimal(self, value, expected):
        # Arrange
        # Act
        result = -value

        # Assert
        self.assertEqual(expected, result)

    @parameterized.expand([
        [1.15, 1, decimal.ROUND_HALF_EVEN, BaseDecimal("1.1")],
        [Decimal("2.14"), 1, decimal.ROUND_UP, BaseDecimal("2.2")],
        [Decimal("2.16"), 1, decimal.ROUND_DOWN, BaseDecimal("2.1")],
        [Decimal("2.15"), 1, decimal.ROUND_HALF_UP, BaseDecimal("2.2")],
        [Decimal("2.15"), 1, decimal.ROUND_HALF_DOWN, BaseDecimal("2.1")],
        [Decimal("2.15"), 1, decimal.ROUND_HALF_EVEN, BaseDecimal("2.1")],
    ])
    def test_rounding_behaviour_with_various_values_returns_expected_decimal(
            self,
            value,
            precision,
            rounding,
            expected,
    ):
        # Arrange
        # Act
        result = BaseDecimal(value, precision, rounding)

        # Assert
        self.assertEqual(expected, result)

    @parameterized.expand([
        [0, BaseDecimal()],
        [1, BaseDecimal("1")],
        [-1, BaseDecimal("-1")],
        ["0", BaseDecimal()],
        ["0.0", BaseDecimal()],
        ["-0.0", BaseDecimal()],
        ["1.0", BaseDecimal("1")],
        ["-1.0", BaseDecimal("-1")],
        [Decimal(), BaseDecimal()],
        [Decimal("1.1"), BaseDecimal("1.1")],
        [Decimal("-1.1"), BaseDecimal("-1.1")],
        [BaseDecimal(), BaseDecimal()],
        [BaseDecimal("1.1"), BaseDecimal("1.1")],
        [BaseDecimal("-1.1"), BaseDecimal("-1.1")],
    ])
    def test_instantiate_with_various_valid_inputs_returns_expected_decimal(self, value, expected):
        # Arrange
        # Act
        decimal_object = BaseDecimal(value)

        # Assert
        self.assertEqual(expected, decimal_object)

    @parameterized.expand([
        [0., 0, BaseDecimal("0")],
        [1., 0, BaseDecimal("1")],
        [-1., 0, BaseDecimal("-1")],
        [1.123, 3, BaseDecimal("1.123")],
        [-1.123, 3, BaseDecimal("-1.123")],
        [1.155, 2, BaseDecimal("1.16")],
    ])
    def test_instantiate_with_various_precisions_returns_expected_decimal(self, value, precision, expected):
        # Arrange
        # Act
        decimal_object = BaseDecimal(value, precision)

        # Assert
        self.assertEqual(expected, decimal_object)
        self.assertEqual(precision, decimal_object.precision)

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
        result = BaseDecimal(value1) == BaseDecimal(value2)

        # Assert
        self.assertEqual(expected, result)

    @parameterized.expand([
        ["0", -0, True],
        ["-0", 0, True],
        ["-1", -1, True],
        ["1", 1, True],
        ["0", 1, False],
        ["-1", 0, False],
        ["-1", -2, False],
        ["1", 2, False],
    ])
    def test_equality_with_various_int_returns_expected_result(self, value1, value2, expected):
        # Arrange
        # Act
        result1 = BaseDecimal(value1) == value2
        result2 = value2 == BaseDecimal(value1)

        # Assert
        self.assertEqual(expected, result1)
        self.assertEqual(expected, result2)

    @parameterized.expand([
        [BaseDecimal(), Decimal(), True],
        [BaseDecimal("0"), Decimal(-0), True],
        [BaseDecimal("1"), Decimal(), False],
    ])
    def test_equality_with_various_decimals_returns_expected_result(self, value1, value2, expected):
        # Arrange
        # Act
        result = value1 == value2

        # Assert
        self.assertEqual(expected, result)

    @parameterized.expand([
        [0, 0, False, True, True, False],
        [1, 0, True, True, False, False],
        [-1, 0, False, False, True, True],
        [BaseDecimal(0), Decimal(0), False, True, True, False],
        [BaseDecimal(1), Decimal(0), True, True, False, False],
        [BaseDecimal(-1), Decimal(0), False, False, True, True],
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
        result1 = BaseDecimal(value1) > BaseDecimal(value2)
        result2 = BaseDecimal(value1) >= BaseDecimal(value2)
        result3 = BaseDecimal(value1) <= BaseDecimal(value2)
        result4 = BaseDecimal(value1) < BaseDecimal(value2)

        # Assert
        self.assertEqual(expected1, result1)
        self.assertEqual(expected2, result2)
        self.assertEqual(expected3, result3)
        self.assertEqual(expected4, result4)

    @parameterized.expand([
        [BaseDecimal(), BaseDecimal(), Decimal, 0],
        [BaseDecimal(), BaseDecimal("1.1"), Decimal, Decimal("1.1")],
        [BaseDecimal(), 0, Decimal, 0],
        [BaseDecimal(), 1, Decimal, 1],
        [0, BaseDecimal(), Decimal, 0],
        [1, BaseDecimal(), Decimal, 1],
        [BaseDecimal(), 0.1, float, 0.1],
        [BaseDecimal(), 1.1, float, 1.1],
        [-1.1, BaseDecimal(), float, -1.1],
        [1.1, BaseDecimal(), float, 1.1],
        [BaseDecimal("1"), BaseDecimal("1.1"), Decimal, Decimal("2.1")],
        [BaseDecimal("1"), Decimal("1.1"), Decimal, Decimal("2.1")],
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
        [BaseDecimal(), BaseDecimal(), Decimal, 0],
        [BaseDecimal(), BaseDecimal("1.1"), Decimal, Decimal("-1.1")],
        [BaseDecimal(), 0, Decimal, 0],
        [BaseDecimal(), 1, Decimal, -1],
        [0, BaseDecimal(), Decimal, 0],
        [1, BaseDecimal("1"), Decimal, 0],
        [BaseDecimal(), 0.1, float, -0.1],
        [BaseDecimal(), 1.1, float, -1.1],
        [0.1, BaseDecimal("1"), float, -0.9],
        [1.1, BaseDecimal("1"), float, 0.10000000000000009],
        [BaseDecimal("1"), BaseDecimal("1.1"), Decimal, Decimal("-0.1")],
        [BaseDecimal("1"), Decimal("1.1"), Decimal, Decimal("-0.1")],
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
        [BaseDecimal(), 0, Decimal, 0],
        [BaseDecimal(1), 1, Decimal, 1],
        [1, BaseDecimal(1), Decimal, 1],
        [2, BaseDecimal(3), Decimal, 6],
        [BaseDecimal(2), 1.0, float, 2],
        [1.1, BaseDecimal(2), float, 2.2],
        [BaseDecimal("1.1"), BaseDecimal("1.1"), Decimal, Decimal("1.21")],
        [BaseDecimal("1.1"), Decimal("1.1"), Decimal, Decimal("1.21")],
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
        [1, BaseDecimal(1), Decimal, 1],
        [1.1, BaseDecimal("1.1"), float, 1],
        [BaseDecimal(), 1, Decimal, 0],
        [BaseDecimal(1), 2, Decimal, Decimal("0.5")],
        [2, BaseDecimal(1), Decimal, Decimal("2.0")],
        [BaseDecimal(2), 1.1, float, 1.8181818181818181],
        [1.1, BaseDecimal(2), float, 1.1 / 2],
        [BaseDecimal("1.1"), BaseDecimal("1.2"), Decimal, Decimal("0.9166666666666666")],
        [BaseDecimal("1.1"), Decimal("1.2"), Decimal, Decimal("0.9166666666666666")],
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

    @parameterized.expand([
        [1, BaseDecimal(1), Decimal, 1],
        [BaseDecimal(), 1, Decimal, 0],
        [BaseDecimal(1), 2, Decimal, Decimal(0)],
        [2, BaseDecimal(1), Decimal, Decimal(2)],
        [2.1, BaseDecimal("1.1"), float, 1],
        [4.4, BaseDecimal("1.1"), float, 4],
        [BaseDecimal("2.1"), 1.1, float, 1],
        [BaseDecimal("4.4"), 1.1, float, 4],
        [BaseDecimal("1.1"), BaseDecimal("1.2"), Decimal, Decimal(0)],
        [BaseDecimal("1.1"), Decimal("1.2"), Decimal, Decimal(0)],
    ])
    def test_floor_division_with_various_types_returns_expected_result(
            self,
            value1,
            value2,
            expected_type,
            expected_value):
        # Arrange
        # Act
        result = value1 // value2

        # Assert
        self.assertEqual(expected_type, type(result))
        self.assertAlmostEqual(expected_value, result)

    @parameterized.expand([
        [1, BaseDecimal(1), Decimal, 0],
        [BaseDecimal(100), 10, Decimal, 0],
        [BaseDecimal(23), 2, Decimal, 1],
        [2, BaseDecimal(1), Decimal, 0],
        [2.1, BaseDecimal("1.1"), float, 1.0],
        [1.1, BaseDecimal("2.1"), float, 1.1],
        [BaseDecimal("2.1"), 1.1, float, 1.0],
        [BaseDecimal("1.1"), 2.1, float, 1.1],
        [BaseDecimal("1.1"), BaseDecimal("0.2"), Decimal, Decimal("0.1")],
        [BaseDecimal("1.1"), Decimal("0.2"), Decimal, Decimal("0.1")],
    ])
    def test_mod_with_various_types_returns_expected_result(
            self,
            value1,
            value2,
            expected_type,
            expected_value):
        # Arrange
        # Act
        result = value1 % value2

        # Assert
        self.assertEqual(expected_type, type(result))
        self.assertAlmostEqual(expected_value, result)

    @parameterized.expand([
        [BaseDecimal(1), BaseDecimal(2), BaseDecimal(2)],
        [BaseDecimal(1), 2, 2],
        [BaseDecimal(1), Decimal(2), Decimal(2)],
    ])
    def test_max_with_various_types_returns_expected_result(
            self,
            value1,
            value2,
            expected,
    ):
        # Arrange
        # Act
        result = max(value1, value2)

        # Assert
        self.assertEqual(expected, result)

    @parameterized.expand([
        [BaseDecimal(1), BaseDecimal(2), BaseDecimal(1)],
        [BaseDecimal(1), 2, BaseDecimal(1)],
        [BaseDecimal(2), Decimal(1), Decimal(1)],
    ])
    def test_min_with_various_types_returns_expected_result(
            self,
            value1,
            value2,
            expected,
    ):
        # Arrange
        # Act
        result = min(value1, value2)

        # Assert
        self.assertEqual(expected, result)

    @parameterized.expand([
        ["1", 1],
        ["1.1", 1],
    ])
    def test_int(self, value, expected):
        # Arrange
        decimal1 = BaseDecimal(value)

        # Act
        # Assert
        self.assertEqual(expected, int(decimal1))

    def test_hash(self):
        # Arrange
        decimal1 = BaseDecimal("1.1")
        decimal2 = BaseDecimal("1.1")

        # Act
        # Assert
        self.assertEqual(int, type(hash(decimal2)))
        self.assertEqual(hash(decimal1), hash(decimal2))

    @parameterized.expand([
        ["0", "0"],
        ["-0", "-0"],
        ["-1", "-1"],
        ["1", "1"],
        ["1.1", "1.1"],
        ["-1.1", "-1.1"],
    ])
    def test_str_with_various_values_returns_expected_string(self, value, expected):
        # Arrange
        # Act
        decimal_object = BaseDecimal(value)

        # Assert
        self.assertEqual(expected, str(decimal_object))

    def test_repr(self):
        # Arrange
        # Act
        result = repr(BaseDecimal("1.1"))

        # Assert
        self.assertEqual("BaseDecimal('1.1')", result)

    @parameterized.expand([
        ["0", BaseDecimal()],
        ["-0", BaseDecimal()],
        ["-1", BaseDecimal("-1")],
        ["1", BaseDecimal("1")],
        ["1.1", BaseDecimal("1.1")],
        ["-1.1", BaseDecimal("-1.1")],
    ])
    def test_as_decimal_with_various_values_returns_expected_value(self, value, expected):
        # Arrange
        # Act
        result = BaseDecimal(value)

        # Assert
        self.assertEqual(expected, result)

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
        result = BaseDecimal(value).as_double()

        # Assert
        self.assertEqual(expected, result)


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
    def test_str_and_to_str(self, value, expected):
        # Arrange
        # Act
        # Assert
        self.assertEqual(expected, Quantity(value).to_str())

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

    def test_instantiate_with_none_value_returns_money_with_zero_amount(self):
        # Arrange
        # Act
        money_zero = Money(None, currency=USD)

        # Assert
        self.assertEqual(0, money_zero.as_decimal())

    @parameterized.expand([
        [0, Money(0, USD)],
        [1, Money("1", USD)],
        [-1, Money("-1", USD)],
        ["0", Money(0, USD)],
        ["0.0", Money(0, USD)],
        ["-0.0", Money(0, USD)],
        ["1.0", Money("1", USD)],
        ["-1.0", Money("-1", USD)],
        [Decimal(), Money(0, USD)],
        [Decimal("1.1"), Money("1.1", USD)],
        [Decimal("-1.1"), Money("-1.1", USD)],
        [BaseDecimal(), Money(0, USD)],
        [BaseDecimal("1.1"), Money("1.1", USD)],
        [BaseDecimal("-1.1"), Money("-1.1", USD)],
    ])
    def test_instantiate_with_various_valid_inputs_returns_expected_money(self, value, expected):
        # Arrange
        # Act
        money = Money(value, USD)

        # Assert
        self.assertEqual(expected, money)

    @parameterized.expand([
        ["0", -0, False, True],
        ["-0", 0, False, True],
        ["-1", -1, False, True],
    ])
    def test_equality_with_different_currencies_returns_false(
            self,
            value1,
            value2,
            expected1,
            expected2,
    ):
        # Arrange
        # Act
        result1 = Money(value1, USD) == Money(value2, BTC)
        result2 = Money(value1, USD) != Money(value2, BTC)

        # Assert
        self.assertEqual(expected1, result1)
        self.assertEqual(expected2, result2)

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

    def test_from_str_with_no_decimal(self):
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
        self.assertEqual("1,000.33 USD", result1.to_str())
        self.assertEqual("5,005.56 USD", result2.to_str())

    def test_hash(self):
        # Arrange
        money0 = Money(0, USD)

        # Act
        # Assert
        self.assertEqual(int, type(hash(money0)))
        self.assertEqual(hash(money0), hash(money0))

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
        self.assertEqual("1,000,000.00 USD", money2.to_str())

    def test_repr(self):
        # Arrange
        money = Money("1.00", USD)

        # Act
        result = repr(money)

        # Assert
        self.assertEqual("Money('1.00', USD)", result)

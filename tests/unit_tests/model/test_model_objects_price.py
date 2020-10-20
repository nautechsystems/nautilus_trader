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

from nautilus_trader.core.decimal import Decimal
from nautilus_trader.model.objects import Price


class PriceTests(unittest.TestCase):

    def test_instantiate_with_none_value_raises_type_error(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(TypeError, Price, None)

    def test_from_float_with_negative_precision_argument_returns_zero_decimal(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(ValueError, Price.from_float, 1.11, -1)

    @parameterized.expand([
        [0, Price()],
        [1, Price("1")],
        ["0", Price()],
        ["0.0", Price()],
        ["-0.0", Price()],
        ["1.0", Price("1")],
        [decimal.Decimal(), Price()],
        [decimal.Decimal("1.1"), Price("1.1")],
        [Decimal(), Price()],
        [Decimal("1.1"), Price("1.1")],
    ])
    def test_instantiate_with_various_valid_inputs_returns_expected_decimal(self, value, expected):
        # Arrange
        # Act
        price = Price(value)

        # Assert
        self.assertEqual(expected, price)

    @parameterized.expand([
        [0., 0, Price("0")],
        [1., 0, Price("1")],
        [1.123, 3, Price("1.123")],
        [1.155, 2, Price("1.16")],
    ])
    def test_from_float_with_various_valid_inputs_returns_expected_decimal(self, value, precision, expected):
        # Arrange
        # Act
        price = Price.from_float(value, precision)

        # Assert
        self.assertEqual(expected, price)

    @parameterized.expand([
        ["0", 0],
        ["-0", 0],
        ["1", 1],
        ["1.1", 1.1],
    ])
    def test_as_double_with_various_values_returns_expected_value(self, value, expected):
        # Arrange
        # Act
        result = Price(value).as_double()

        # Assert
        self.assertEqual(expected, result)

    @parameterized.expand([
        ["0", -0, True],
        ["-0", 0, True],
        ["1", 1, True],
        ["1.1", "1.1", True],
        ["0", 1, False],
        ["1", 2, False],
        ["1.1", "1.12", False],
    ])
    def test_equality_with_various_values_returns_expected_result(self, value1, value2, expected):
        # Arrange
        # Act
        result = Price(value1) == Price(value2)

        # Assert
        self.assertEqual(expected, result)

    @parameterized.expand([
        [0, 0, False, True, True, False],
        [2, 1, True, True, False, False],
        [1, 2, False, False, True, True],
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
        result1 = Price(value1) > Price(value2)
        result2 = Price(value1) >= Price(value2)
        result3 = Price(value1) <= Price(value2)
        result4 = Price(value1) < Price(value2)

        # Assert
        self.assertEqual(expected1, result1)
        self.assertEqual(expected2, result2)
        self.assertEqual(expected3, result3)
        self.assertEqual(expected4, result4)

    @parameterized.expand([
        [Price(), Price(), Decimal, 0],
        [Price(), Price("1.1"), Decimal, Decimal("1.1")],
        [Price(), 0, Decimal, 0],
        [Price(), 1, Decimal, 1],
        [Price(), 0.0, float, 0],
        [Price(), 1.0, float, 1.0],
        [Price("1"), Decimal("1.1"), Decimal, Decimal("2.1")],
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
        [Price(), Price(), Decimal, 0],
        [Price(), Price("1.1"), Decimal, Decimal("-1.1")],
        [Price(), 0, Decimal, 0],
        [Price(), 1, Decimal, -1],
        [Price(), 0.0, float, 0],
        [Price(), 1.0, float, -1.0],
        [Price("1"), Decimal("1.1"), Decimal, Decimal("-0.1")],
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
        [Price(), 0, Decimal, 0],
        [Price(1), 1, Decimal, 1],
        [Price(2), 1.0, float, 2],
        [Price("1.1"), Decimal("1.1"), Decimal, Decimal("1.21")],
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
        [Price(), 1, Decimal, 0],
        [Price(1), 2, Decimal, 0.5],
        [Price(2), 1.0, float, 2],
        [Price("1.1"), Decimal("1.2"), Decimal, 0.9166666666666666],
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

    def test_str(self):
        # Arrange
        price = Price("1.00000")

        # Act
        result = str(price)

        # Assert
        self.assertEqual("1.00000", result)
        self.assertEqual("1.00000", price.to_string())

    def test_repr(self):
        # Arrange
        price = Price.from_float(1.00000, 5)

        # Act
        result = repr(price)

        print(repr(price))
        # Assert
        self.assertTrue(result.startswith("<Price('1.00000') object at"))

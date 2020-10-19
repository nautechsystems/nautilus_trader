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
from nautilus_trader.model.objects import Decimal


class DecimalTests(unittest.TestCase):

    def test_instantiate_with_none_value_raises_type_error(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(TypeError, Decimal, None)

    def test_instantiate_with_float_value_raises_type_error(self):  # User should use .from_float()
        # Arrange
        # Act
        # Assert
        self.assertRaises(TypeError, Decimal, 1.1)

    def test_from_float_with_negative_precision_argument_returns_zero_decimal(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(ValueError, Decimal.from_float, 1.11, -1)

    @parameterized.expand([
        [0, Decimal()],
        [1, Decimal("1")],
        [-1, Decimal("-1")],
        ["0", Decimal()],
        ["0.0", Decimal()],
        ["-0.0", Decimal()],
        ["1.0", Decimal("1")],
        ["-1.0", Decimal("-1")],
        [decimal.Decimal(), Decimal()],
        [decimal.Decimal("1.1"), Decimal("1.1")],
        [decimal.Decimal("-1.1"), Decimal("-1.1")],
        [Fraction(), Decimal()],
        [Fraction("1.1"), Decimal("1.1")],
        [Fraction("-1.1"), Decimal("-1.1")],
    ])
    def test_instantiate_with_various_valid_inputs_returns_expected_decimal(self, value, expected):
        # Arrange
        # Act
        decimal_object = Decimal(value)

        # Assert
        self.assertEqual(expected, decimal_object)

    @parameterized.expand([
        [0., 0, Decimal("0")],
        [1., 0, Decimal("1")],
        [-1., 0, Decimal("-1")],
        [1.123, 3, Decimal("1.123")],
        [-1.123, 3, Decimal("-1.123")],
        [1.155, 2, Decimal("1.16")],
    ])
    def test_from_float_with_various_valid_inputs_returns_expected_decimal(self, value, precision, expected):
        # Arrange
        # Act
        decimal_object = Decimal.from_float(value, precision)

        # Assert
        self.assertEqual(expected, decimal_object)

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
        result = Decimal(value).as_double()

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
        result = Decimal(value1) == Decimal(value2)

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
        result1 = Decimal(value1) > Decimal(value2)
        result2 = Decimal(value1) >= Decimal(value2)
        result3 = Decimal(value1) <= Decimal(value2)
        result4 = Decimal(value1) < Decimal(value2)

        # Assert
        self.assertEqual(expected1, result1)
        self.assertEqual(expected2, result2)
        self.assertEqual(expected3, result3)
        self.assertEqual(expected4, result4)

    @parameterized.expand([
        [Decimal(), Decimal(), Fraction, 0],
        [Decimal(), Decimal("1.1"), Fraction, Fraction("1.1")],
        [Decimal(), 0, Fraction, 0],
        [Decimal(), 1, Fraction, 1],
        [Decimal(), 0.0, float, 0],
        [Decimal(), 1.0, float, 1.0],
        [Decimal("1"), Fraction("1.1"), Fraction, Fraction("2.1")],
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
        [Decimal(), Decimal(), Fraction, 0],
        [Decimal(), Decimal("1.1"), Fraction, Fraction("-1.1")],
        [Decimal(), 0, Fraction, 0],
        [Decimal(), 1, Fraction, -1],
        [Decimal(), 0.0, float, 0],
        [Decimal(), 1.0, float, -1.0],
        [Decimal("1"), Fraction("1.1"), Fraction, Fraction("-0.1")],
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
        [Decimal(), 0, Fraction, 0],
        [Decimal(1), 1, Fraction, 1],
        [Decimal(2), 1.0, float, 2],
        [Decimal("1.1"), Fraction("1.1"), Fraction, Fraction("1.21")],
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
        [Decimal(), 1, Fraction, 0],
        [Decimal(1), 2, Fraction, 0.5],
        [Decimal(2), 1.0, float, 2],
        [Decimal("1.1"), Fraction("1.2"), Fraction, 0.9166666666666666],
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
        ["0", "0"],
        ["-0", "0"],
        ["-1", "-1"],
        ["1", "1"],
        ["1.1", "1.1"],
        ["-1.1", "-1.1"],
    ])
    def test_str_and_as_string_with_various_values_returns_expected_string(self, value, expected):
        # Arrange
        # Act
        decimal_object = Decimal(value)

        # Assert
        self.assertEqual(expected, str(decimal_object))
        self.assertEqual(expected, decimal_object.to_string())

    def test_repr(self):
        # Arrange
        # Act
        result = repr(Decimal("1.1"))

        # Assert
        self.assertTrue(result.startswith("<Decimal('1.1') object at "))
        self.assertTrue(result.endswith(">"))

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

from nautilus_trader.model.objects import Decimal
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.quicktions import Fraction
from tests.test_kit.stubs import TestStubs

AUDUSD_FXCM = TestStubs.symbol_audusd_fxcm()
GBPUSD_FXCM = TestStubs.symbol_gbpusd_fxcm()


class QuantityTests(unittest.TestCase):

    def test_instantiate_with_none_value_raises_type_error(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(TypeError, Quantity, None)

    def test_instantiate_with_negative_integer_raises_exception(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(ValueError, Quantity, -1)

    def test_instantiate_with_float_value_raises_type_error(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(TypeError, Quantity, 1.1)

    def test_from_float_with_negative_precision_argument_returns_zero_decimal(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(ValueError, Quantity.from_float, 1.11, -1)

    @parameterized.expand([
        [0, Quantity()],
        [1, Quantity("1")],
        ["0", Quantity()],
        ["0.0", Quantity()],
        ["1.0", Quantity("1")],
        [decimal.Decimal(), Quantity()],
        [decimal.Decimal("1.1"), Quantity("1.1")],
    ])
    def test_instantiate_with_various_valid_inputs_returns_expected_decimal(self, value, expected):
        # Arrange
        # Act
        quantity = Decimal(value)

        # Assert
        self.assertEqual(expected, quantity)

    @parameterized.expand([
        [0., 0, Quantity("0")],
        [1., 0, Quantity("1")],
        [1.123, 3, Quantity("1.123")],
        [1.155, 2, Quantity("1.16")],
    ])
    def test_from_float_with_various_valid_inputs_returns_expected_decimal(self, value, precision, expected):
        # Arrange
        # Act
        quantity = Decimal.from_float(value, precision)

        # Assert
        self.assertEqual(expected, quantity)

    @parameterized.expand([
        ["0", 0],
        ["-0", 0],
        ["1", 1],
        ["1.1", 1.1],
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
        ["1", 1, True],
        ["1.1", "1.1", True],
        ["0", 1, False],
        ["1", 2, False],
        ["1.1", "1.12", False],
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
        [Quantity(), Quantity(), Fraction, 0],
        [Quantity(), Quantity("1.1"), Fraction, Fraction("1.1")],
        [Quantity(), 0, Fraction, 0],
        [Quantity(), 1, Fraction, 1],
        [Quantity(), 0.0, float, 0],
        [Quantity(), 1.0, float, 1.0],
        [Quantity("1"), Fraction("1.1"), Fraction, Fraction("2.1")],
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
        [Quantity(), Quantity(), Fraction, 0],
        [Quantity(), Quantity("1.1"), Fraction, Fraction("-1.1")],
        [Quantity(), 0, Fraction, 0],
        [Quantity(), 1, Fraction, -1],
        [Quantity(), 0.0, float, 0],
        [Quantity(), 1.0, float, -1.0],
        [Quantity("1"), Fraction("1.1"), Fraction, Fraction("-0.1")],
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
        [Quantity(), 0, Fraction, 0],
        [Quantity(1), 1, Fraction, 1],
        [Quantity(2), 1.0, float, 2],
        [Quantity("1.1"), Fraction("1.1"), Fraction, Fraction("1.21")],
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
        [Quantity(), 1, Fraction, 0],
        [Quantity(1), 2, Fraction, 0.5],
        [Quantity(2), 1.0, float, 2],
        [Quantity("1.1"), Fraction("1.2"), Fraction, 0.9166666666666666],
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
        quantity = Decimal(value)

        # Assert
        self.assertEqual(expected, str(quantity))
        self.assertEqual(expected, quantity.to_string())

    def test_str(self):
        # Arrange
        # Act
        # Assert
        self.assertEqual("0", str(Quantity("0")))
        self.assertEqual("1000", str(Quantity("1000")))
        self.assertEqual("10.05", Quantity("10.05").to_string())
        self.assertEqual("1K", Quantity(1000).to_string_formatted())
        self.assertEqual("1K", Quantity("1000").to_string_formatted())
        self.assertEqual("120,100", Quantity("120100").to_string_formatted())
        self.assertEqual("200K", Quantity("200000").to_string_formatted())
        self.assertEqual("1M", Quantity("1000000").to_string_formatted())
        self.assertEqual("1M", Quantity(1000000).to_string_formatted())
        self.assertEqual("2.5M", Quantity("2500000").to_string_formatted())
        self.assertEqual("1,111,111", Quantity("1111111").to_string_formatted())
        self.assertEqual("2.523M", Quantity("2523000").to_string_formatted())
        self.assertEqual("100M", Quantity("100000000").to_string_formatted())

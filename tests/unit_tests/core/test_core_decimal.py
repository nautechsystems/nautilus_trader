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

from nautilus_trader.core.decimal import Decimal


class DecimalTests(unittest.TestCase):

    def test_initialized_with_no_value_returns_valid_decimal(self):
        # Arrange
        # Act
        result = Decimal()

        # Assert
        self.assertEqual(0, result)
        self.assertEqual(0, result.precision)
        self.assertEqual(decimal.Decimal("0"), result.as_decimal())
        self.assertEqual(0, result.as_double())

    def test_initialized_with_valid_inputs(self):
        # Arrange
        # Act
        result0 = Decimal(1.0, 1)
        result1 = Decimal(1.0, 2)
        result2 = Decimal(-1.001, 3)
        result3 = Decimal(1.0005, 3)
        result4 = Decimal(100)

        # Assert
        self.assertEqual(decimal.Decimal("1"), result0.as_decimal())
        self.assertEqual(decimal.Decimal("1.00"), result1.as_decimal())
        self.assertEqual(decimal.Decimal("-1.001"), result2.as_decimal())
        self.assertEqual(decimal.Decimal("1.001"), result3.as_decimal())  # Rounds up
        self.assertEqual(100, result4)
        self.assertEqual(1, result0.as_double())
        self.assertEqual(1, result0.as_double())
        self.assertEqual(-1.001, result2.as_double())
        self.assertEqual(1.001, result3.as_double())
        self.assertEqual("1.0", result0.to_string())
        self.assertEqual("1.00", result1.to_string())
        self.assertEqual("-1.001", result2.to_string())
        self.assertEqual("1.001", result3.to_string())

    def test_initialized_with_many_scientific_notation_returns_zero(self):
        # Arrange
        # Act
        result1 = Decimal(0E-30)
        result2 = Decimal(-0E-33)

        # Assert
        self.assertEqual(0.0, result1.as_double())
        self.assertEqual(0.0, result2.as_double())
        self.assertEqual(decimal.Decimal("0"), result1.as_decimal())
        self.assertEqual(decimal.Decimal("0"), result2.as_decimal())

    def test_decimal_initialized_with_negative_precision_raises_exception(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(ValueError, Decimal, 1.00000, -1)

    def test_decimal_addition(self):
        # Arrange
        # Act
        result0 = Decimal(1.00001, 5) + 0.00001
        result1 = Decimal(1.00001, 5) + Decimal(0.00001, 5)
        result3 = Decimal(1.00001, 5).add_as_decimal(Decimal(0.00001, 5))
        result4 = Decimal(1.00001, 5).add_as_decimal(Decimal(0.5, 1))

        # Assert
        self.assertEqual(float, type(result0))
        self.assertEqual(float, type(result1))
        self.assertEqual(Decimal, type(result3))
        self.assertEqual(Decimal, type(result4))
        self.assertEqual(1.0000200000000001, result0)
        self.assertEqual(1.0000200000000001, result1)
        self.assertEqual(Decimal(1.00002, 5), result3)
        self.assertEqual(Decimal(1.50001, 5), result4)

    def test_decimal_subtraction(self):
        # Arrange
        # Act
        result0 = Decimal(1.00001, 5) - 0.00001
        result1 = Decimal(1.00001, 5) - Decimal(0.00001, 5)
        result3 = Decimal(1.00001, 5).sub_as_decimal(Decimal(0.00001, 5))
        result4 = Decimal(1.00001, 5).sub_as_decimal(Decimal(0.5, 1))

        # Assert
        self.assertEqual(float, type(result0))
        self.assertEqual(float, type(result1))
        self.assertEqual(Decimal, type(result3))
        self.assertEqual(Decimal, type(result4))
        self.assertEqual(1.0, result0)
        self.assertEqual(1.0, result1)
        self.assertEqual(result0, result1)
        self.assertEqual(Decimal(1.00000, 5), result3)
        self.assertEqual(Decimal(0.50001, 5), result4)

    def test_decimal_division(self):
        # Arrange
        # Act
        result0 = Decimal(1.00001, 5) / 2.0
        result1 = Decimal(1.00001, 5) / Decimal(0.5000, 5)

        # Assert
        self.assertEqual(float, type(result0))
        self.assertEqual(float, type(result1))
        self.assertEqual(0.500005, result0)
        self.assertEqual(2.00002, result1)
        self.assertEqual(result0, Decimal(1.00001, 5) / Decimal(2.0, 1))

    def test_decimal_multiplication(self):
        # Arrange
        # Act
        result0 = Decimal(1.00001, 5) * 2.0
        result1 = Decimal(1.00001, 5) * Decimal(1.5000, 5)

        # Assert
        self.assertEqual(float, type(result0))
        self.assertEqual(float, type(result1))
        self.assertEqual(2.00002, result0)
        self.assertEqual(1.500015, result1)

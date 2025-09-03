# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

import math
import pickle
from decimal import Decimal

import pytest

from nautilus_trader.model import convert_to_raw_int
from nautilus_trader.model.objects import FIXED_PRECISION
from nautilus_trader.model.objects import QUANTITY_MAX
from nautilus_trader.model.objects import Quantity


class TestQuantity:
    def test_instantiate_with_nan_raises_value_error(self):
        # Arrange, Act, Assert
        with pytest.raises(ValueError):
            Quantity(math.nan, precision=0)

    def test_instantiate_with_none_value_raises_type_error(self):
        # Arrange, Act, Assert
        with pytest.raises(TypeError):
            Quantity(None)

        with pytest.raises(TypeError):
            Quantity(None, precision=0)

    def test_instantiate_with_negative_precision_raises_overflow_error(self):
        # Arrange, Act, Assert
        with pytest.raises(OverflowError):
            Quantity(1.0, precision=-1)

    def test_instantiate_with_precision_over_maximum_raises_value_error(self):
        # Arrange, Act, Assert
        with pytest.raises(ValueError):
            Quantity(1.0, precision=FIXED_PRECISION + 1)

    def test_instantiate_with_value_exceeding_limit_raises_value_error(self):
        # Arrange, Act, Assert
        with pytest.raises(ValueError):
            Quantity(QUANTITY_MAX + 1, precision=0)

    def test_instantiate_base_decimal_from_int(self):
        # Arrange, Act
        Quantity(1, precision=1)

    def test_instantiate_base_decimal_from_float(self):
        # Arrange
        value = 1.12300
        precision = 5

        # Act
        result = Quantity(value, precision=precision)

        # Assert
        assert result.raw == convert_to_raw_int(value, precision)
        assert str(result) == "1.12300"

    def test_instantiate_base_decimal_from_decimal(self):
        # Arrange, Act
        result = Quantity(Decimal("1.23"), precision=1)

        # Assert
        assert str(result) == "1.2"

    def test_instantiate_base_decimal_from_str(self):
        # Arrange, Act
        result = Quantity.from_str("1.23")

        # Assert
        assert result.raw == convert_to_raw_int(1.23, 2)
        assert str(result) == "1.23"

    @pytest.mark.parametrize(
        ("value", "precision", "expected"),
        [
            [Quantity(2.15, precision=2), 0, Decimal("2")],
            [Quantity(2.15, precision=2), 1, Decimal("2.2")],
            [Quantity(2.255, precision=3), 2, Decimal("2.26")],
        ],
    )
    def test_round_with_various_digits_returns_expected_decimal(self, value, precision, expected):
        # Arrange, Act
        result = round(value, precision)

        # Assert
        assert result == expected

    @pytest.mark.parametrize(
        ("value", "expected"),
        [
            [Quantity(-0, precision=0), Decimal("0")],
            [Quantity(0, precision=0), Decimal("0")],
            [Quantity(1, precision=0), Decimal("1")],
        ],
    )
    def test_abs_with_various_values_returns_expected_decimal(self, value, expected):
        # Arrange, Act
        result = abs(value)

        # Assert
        assert result == expected

    @pytest.mark.parametrize(
        ("value", "expected"),
        [
            [Quantity(1, precision=0), Decimal("-1")],
            [Quantity(0, precision=0), Decimal("0")],
        ],
    )
    def test_neg_with_various_values_returns_expected_decimal(self, value, expected):
        # Arrange, Act
        result = -value

        # Assert
        assert result == expected

    @pytest.mark.parametrize(
        ("value", "expected"),
        [
            [0, Quantity(0, precision=0)],
            [1, Quantity(1, precision=0)],
            [Decimal(0), Quantity(0, precision=0)],
            [Decimal("1.1"), Quantity(1.1, precision=1)],
            [Quantity(0, precision=0), Quantity(0, precision=0)],
            [Quantity(1.1, precision=1), Quantity(1.1, precision=1)],
        ],
    )
    def test_instantiate_with_various_valid_inputs_returns_expected_decimal(self, value, expected):
        # Arrange, Act
        decimal_object = Quantity(value, 2)

        # Assert
        assert decimal_object == expected

    @pytest.mark.parametrize(
        ("value", "precision", "expected"),
        [
            [0.0, 0, Quantity(0, precision=0)],
            [1.0, 0, Quantity(1, precision=0)],
            [1.123, 3, Quantity(1.123, precision=3)],
            [1.155, 2, Quantity(1.16, precision=2)],
        ],
    )
    def test_instantiate_with_various_precisions_returns_expected_decimal(
        self,
        value,
        precision,
        expected,
    ):
        # Arrange, Act
        decimal_object = Quantity(value, precision)

        # Assert
        assert decimal_object == expected
        assert decimal_object.precision == precision

    @pytest.mark.parametrize(
        ("value1", "value2", "expected"),
        [
            [0, -0, True],
            [-0, 0, True],
            [1, 1, True],
            [1.1, 1.1, True],
            [0, 1, False],
            [1, 2, False],
            [1.1, 1.12, False],
        ],
    )
    def test_equality_with_various_values_returns_expected_result(self, value1, value2, expected):
        # Arrange, Act
        result = Quantity(value1, 2) == Quantity(value2, 2)

        # Assert
        assert result == expected

    @pytest.mark.parametrize(
        ("value1", "value2", "expected"),
        [
            [0, -0, True],
            [-0, 0, True],
            [1, 1, True],
            [0, 1, False],
            [1, 2, False],
        ],
    )
    def test_equality_with_various_int_returns_expected_result(self, value1, value2, expected):
        # Arrange, Act
        result1 = Quantity(value1, 0) == value2
        result2 = value2 == Quantity(value1, 0)

        # Assert
        assert result1 == expected
        assert result2 == expected

    @pytest.mark.parametrize(
        ("value1", "value2", "expected"),
        [
            [Quantity(0, precision=0), Decimal(0), True],
            [Quantity(0, precision=0), Decimal(-0), True],
            [Quantity(1, precision=0), Decimal(0), False],
        ],
    )
    def test_equality_with_various_decimals_returns_expected_result(self, value1, value2, expected):
        # Arrange, Act
        result = value1 == value2

        # Assert
        assert result == expected

    @pytest.mark.parametrize(
        ("value1", "value2", "expected1", "expected2", "expected3", "expected4"),
        [
            [0, 0, False, True, True, False],
            [1, 0, True, True, False, False],
        ],
    )
    def test_comparisons_with_various_values_returns_expected_result(
        self,
        value1,
        value2,
        expected1,
        expected2,
        expected3,
        expected4,
    ):
        # Arrange, Act, Assert
        assert (Quantity(value1, precision=0) > Quantity(value2, precision=0)) == expected1
        assert (Quantity(value1, precision=0) >= Quantity(value2, precision=0)) == expected2
        assert (Quantity(value1, precision=0) <= Quantity(value2, precision=0)) == expected3
        assert (Quantity(value1, precision=0) < Quantity(value2, precision=0)) == expected4

    @pytest.mark.parametrize(
        ("value1", "value2", "expected_type", "expected_value"),
        [
            [Quantity(0, precision=0), Quantity(0, precision=0), Decimal, 0],
            [
                Quantity(0, precision=0),
                Quantity(1.1, precision=1),
                Decimal,
                Decimal("1.1"),
            ],
            [Quantity(0, precision=0), 0, Decimal, 0],
            [Quantity(0, precision=0), 1, Decimal, 1],
            [0, Quantity(0, precision=0), Decimal, 0],
            [1, Quantity(0, precision=0), Decimal, 1],
            [Quantity(0, precision=0), 0.1, float, 0.1],
            [Quantity(0, precision=0), 1.1, float, 1.1],
            [-1.1, Quantity(0, precision=0), float, -1.1],
            [1.1, Quantity(0, precision=0), float, 1.1],
            [
                Quantity(1, precision=0),
                Quantity(1.1, precision=1),
                Decimal,
                Decimal("2.1"),
            ],
            [Quantity(1, precision=0), Decimal("1.1"), Decimal, Decimal("2.1")],
        ],
    )
    def test_addition_with_various_types_returns_expected_result(
        self,
        value1,
        value2,
        expected_type,
        expected_value,
    ):
        # Arrange, Act
        result = value1 + value2

        # Assert
        assert isinstance(result, expected_type)
        assert result == expected_value

    @pytest.mark.parametrize(
        ("value1", "value2", "expected_type", "expected_value"),
        [
            [Quantity(0, precision=0), Quantity(0, precision=0), Decimal, 0],
            [
                Quantity(0, precision=0),
                Quantity(1.1, precision=1),
                Decimal,
                Decimal("-1.1"),
            ],
            [Quantity(0, precision=0), 0, Decimal, 0],
            [Quantity(0, precision=0), 1, Decimal, -1],
            [0, Quantity(0, precision=0), Decimal, 0],
            [1, Quantity(1, precision=0), Decimal, 0],
            [Quantity(0, precision=0), 0.1, float, -0.1],
            [Quantity(0, precision=0), 1.1, float, -1.1],
            [0.1, Quantity(1, precision=0), float, -0.9],
            [1.1, Quantity(1, precision=0), float, 0.10000000000000009],
            [
                Quantity(1, precision=0),
                Quantity(1.1, precision=1),
                Decimal,
                Decimal("-0.1"),
            ],
            [Quantity(1, precision=0), Decimal("1.1"), Decimal, Decimal("-0.1")],
        ],
    )
    def test_subtraction_with_various_types_returns_expected_result(
        self,
        value1,
        value2,
        expected_type,
        expected_value,
    ):
        # Arrange, Act
        result = value1 - value2

        # Assert
        assert isinstance(result, expected_type)
        assert result == expected_value

    @pytest.mark.parametrize(
        ("value1", "value2", "expected_type", "expected_value"),
        [
            [Quantity(0, 0), 0, Decimal, 0],
            [Quantity(1, 0), 1, Decimal, 1],
            [1, Quantity(1, 0), Decimal, 1],
            [2, Quantity(3, 0), Decimal, 6],
            [Quantity(2, 0), 1.0, float, 2],
            [1.1, Quantity(2, 0), float, 2.2],
            [Quantity(1.1, 1), Quantity(1.1, 1), Decimal, Decimal("1.21")],
            [Quantity(1.1, 1), Decimal("1.1"), Decimal, Decimal("1.21")],
        ],
    )
    def test_multiplication_with_various_types_returns_expected_result(
        self,
        value1,
        value2,
        expected_type,
        expected_value,
    ):
        # Arrange, Act
        result = value1 * value2

        # Assert
        assert isinstance(result, expected_type)
        assert result == expected_value

    @pytest.mark.parametrize(
        ("value1", "value2", "expected_type", "expected_value"),
        [
            [1, Quantity(1, 0), Decimal, 1],
            [1.1, Quantity(1.1, 1), float, 1],
            [Quantity(0, 0), 1, Decimal, 0],
            [Quantity(1, 0), 2, Decimal, Decimal("0.5")],
            [2, Quantity(1, 0), Decimal, Decimal("2.0")],
            [Quantity(2, 0), 1.1, float, 1.8181818181818181],
            [1.1, Quantity(2, 0), float, 1.1 / 2],
            [
                Quantity(1.1, 1),
                Quantity(1.2, 1),
                Decimal,
                Decimal("0.9166666666666666666666666667"),
            ],
            [
                Quantity(1.1, 1),
                Decimal("1.2"),
                Decimal,
                Decimal("0.9166666666666666666666666667"),
            ],
        ],
    )
    def test_division_with_various_types_returns_expected_result(
        self,
        value1,
        value2,
        expected_type,
        expected_value,
    ):
        # Arrange, Act
        result = value1 / value2

        # Assert
        assert isinstance(result, expected_type)
        assert result == expected_value

    @pytest.mark.parametrize(
        ("value1", "value2", "expected_type", "expected_value"),
        [
            [1, Quantity(1, 0), Decimal, 1],
            [Quantity(0, 0), 1, Decimal, 0],
            [Quantity(1, 0), 2, Decimal, Decimal(0)],
            [2, Quantity(1, 0), Decimal, Decimal(2)],
            [2.1, Quantity(1.1, 1), float, 1],
            [4.4, Quantity(1.1, 1), float, 4],
            [Quantity(2.1, 1), 1.1, float, 1],
            [Quantity(4.4, 1), 1.1, float, 4],
            [Quantity(1.1, 1), Quantity(1.2, 1), Decimal, Decimal(0)],
            [Quantity(1.1, 1), Decimal("1.2"), Decimal, Decimal(0)],
        ],
    )
    def test_floor_division_with_various_types_returns_expected_result(
        self,
        value1,
        value2,
        expected_type,
        expected_value,
    ):
        # Arrange, Act
        result = value1 // value2

        # Assert
        assert type(result) is expected_type
        assert result == expected_value

    @pytest.mark.parametrize(
        ("value1", "value2", "expected_type", "expected_value"),
        [
            [Quantity(100, 0), 10, Decimal, 0],
            [Quantity(23, 0), 2, Decimal, 1],
            [2.1, Quantity(1.1, 1), float, 1.0],
            [1.1, Quantity(2.1, 1), float, 1.1],
            [Quantity(2.1, 1), 1.1, float, 1.0],
            [Quantity(1.1, 1), 2.1, float, 1.1],
            [Quantity(1.1, 1), Decimal("0.2"), Decimal, Decimal("0.1")],
        ],
    )
    def test_mod_with_various_types_returns_expected_result(
        self,
        value1,
        value2,
        expected_type,
        expected_value,
    ):
        # Arrange, Act
        result = value1 % value2

        # Assert
        assert type(result) is expected_type
        assert result == expected_value

    @pytest.mark.parametrize(
        ("value1", "value2", "expected"),
        [
            [Quantity(1, 0), Quantity(2, 0), Quantity(2, 0)],
            [Quantity(1, 0), 2, 2],
            [Quantity(1, 0), Decimal(2), Decimal(2)],
        ],
    )
    def test_max_with_various_types_returns_expected_result(
        self,
        value1,
        value2,
        expected,
    ):
        # Arrange, Act
        result = max(value1, value2)

        # Assert
        assert result == expected

    @pytest.mark.parametrize(
        ("value1", "value2", "expected"),
        [
            [Quantity(1, 0), Quantity(2, 0), Quantity(1, 0)],
            [Quantity(1, 0), 2, Quantity(1, 0)],
            [Quantity(2, 0), Decimal(1), Decimal(1)],
        ],
    )
    def test_min_with_various_types_returns_expected_result(
        self,
        value1,
        value2,
        expected,
    ):
        # Arrange, Act
        result = min(value1, value2)

        # Assert
        assert result == expected

    @pytest.mark.parametrize(
        ("value", "expected"),
        [["1", 1], ["1.1", 1]],
    )
    def test_int(self, value, expected):
        # Arrange
        decimal1 = Quantity.from_str(value)

        # Act, Assert
        assert int(decimal1) == expected

    def test_hash(self):
        # Arrange
        decimal1 = Quantity(1.1, 1)
        decimal2 = Quantity(1.1, 1)

        # Act, Assert
        assert isinstance(hash(decimal2), int)
        assert hash(decimal1) == hash(decimal2)

    @pytest.mark.parametrize(
        ("value", "precision", "expected"),
        [
            [0, 0, "0"],
            [-0, 0, "0"],
            [1, 0, "1"],
            [1.1, 1, "1.1"],
        ],
    )
    def test_str_with_various_values_returns_expected_string(
        self,
        value,
        precision,
        expected,
    ):
        # Arrange, Act
        decimal_object = Quantity(value, precision=precision)

        # Assert
        assert str(decimal_object) == expected

    def test_repr(self):
        # Arrange, Act
        result = repr(Quantity(1.1, 1))

        # Assert
        assert result == "Quantity(1.1)"

    @pytest.mark.parametrize(
        ("value", "precision", "expected"),
        [
            [0, 0, Quantity(0, 0)],
            [-0, 0, Quantity(0, 0)],
            [1, 0, Quantity(1, 0)],
            [1.1, 1, Quantity(1.1, 1)],
        ],
    )
    def test_as_decimal_with_various_values_returns_expected_value(
        self,
        value,
        precision,
        expected,
    ):
        # Arrange, Act
        result = Quantity(value, precision=precision)

        # Assert
        assert result == expected

    @pytest.mark.parametrize(
        ("value", "expected"),
        [[0, 0], [-0, 0], [1, 1], [1.1, 1.1]],
    )
    def test_as_double_with_various_values_returns_expected_value(self, value, expected):
        # Arrange, Act
        result = Quantity(value, 1).as_double()

        # Assert
        assert result == expected

    def test_calling_new_returns_an_expected_zero_quantity(self):
        # Arrange, Act
        new_qty = Quantity.__new__(Quantity, 1, 1)

        # Assert
        assert new_qty == 0

    def test_from_raw_returns_expected_quantity(self):
        # Arrange
        value = 1000
        precision = 3

        # Act
        raw_value = convert_to_raw_int(value, precision)
        qty1 = Quantity.from_raw(raw_value, precision)
        qty2 = Quantity(value, precision)

        # Assert
        assert qty1 == qty2
        assert str(qty1) == "1000.000"
        assert qty1.precision == precision

    def test_zero_returns_zero_quantity(self):
        # Arrange, Act
        qty = Quantity.zero()

        # Assert
        assert qty == 0
        assert str(qty) == "0"
        assert qty.precision == 0

    def test_from_int_returns_expected_value(self):
        # Arrange, Act
        qty = Quantity.from_int(1_000)

        # Assert
        assert qty == 1000
        assert str(qty) == "1000"
        assert qty.precision == 0

    def test_from_str_returns_expected_value(self):
        # Arrange, Act
        qty = Quantity.from_str("0.511")

        # Assert
        assert qty == Quantity(0.511, precision=3)
        assert str(qty) == "0.511"
        assert qty.precision == 3

    @pytest.mark.parametrize(
        ("value", "expected_str", "expected_precision"),
        [
            # Scientific notation tests
            ["1e6", "1000000", 0],
            ["1E6", "1000000", 0],
            ["2.5e4", "25000.000", 3],
            ["3.5E-2", "0.04", 2],
            ["1.23456e-3", "0.001", 3],
            ["7.89E1", "78.9000", 4],
            # Underscore handling
            ["1_000", "1000", 0],
            ["1_000.25", "1000.25", 2],
            ["9_876_543.21", "9876543.21", 2],
            ["0.000_123", "0.000123", 6],
            # Combined underscores and scientific notation
            ["1_000e2", "100000", 0],
            ["2_345.6e-3", "2.346", 3],
            # Edge cases for precision
            [
                "0.123456789012345" if FIXED_PRECISION > 9 else "0.123456789",
                "0.123456789012345" if FIXED_PRECISION > 9 else "0.123456789",
                min(15, FIXED_PRECISION),
            ],
            ["987654321.123456789", "987654321.123456789", 9],  # Full precision preserved
            # Rounding behavior verification
            ["2.115", "2.115", 3],
            ["2.125", "2.125", 3],
            ["2.135", "2.135", 3],
            ["2.145", "2.145", 3],
            ["2.155", "2.155", 3],
            # Zero representations
            ["0e0", "0", 0],
            ["0.0e5", "0.000", 3],
            ["0E-3", "0.000", 3],
            # Small numbers
            [
                "1e-15" if FIXED_PRECISION > 9 else "1e-9",
                "0.000000000000001" if FIXED_PRECISION > 9 else "0.000000001",
                min(15, FIXED_PRECISION) if FIXED_PRECISION > 9 else 9,
            ],
        ],
    )
    def test_from_str_comprehensive(self, value, expected_str, expected_precision):
        # Arrange, Act
        qty = Quantity.from_str(value)

        # Assert
        assert str(qty) == expected_str
        assert qty.precision == expected_precision

    @pytest.mark.parametrize(
        "invalid_input",
        [
            "not_a_number",
            "1.2.3",
            "++1",
            "--1",
            "-1",  # Negative values not allowed for Quantity
            "-0.5",
            "-1e3",
            "1e",
            "e10",
            "1e1e1",
            "",
            "nan",
            "inf",
            "-inf",
            "1e1000",  # Overflow
        ],
    )
    def test_from_str_invalid_input_raises_value_error(self, invalid_input):
        # Arrange, Act, Assert
        with pytest.raises(Exception):  # Various exceptions can be raised for invalid input
            Quantity.from_str(invalid_input)

    def test_from_str_with_negative_value_raises_value_error(self):
        with pytest.raises(ValueError, match="invalid negative quantity"):
            Quantity.from_str("-1.0")
        with pytest.raises(ValueError, match="invalid negative quantity"):
            Quantity.from_str("-0.001")

    def test_from_str_with_precision_exceeding_max_raises_value_error(self):
        if FIXED_PRECISION <= 9:
            # On Windows with 9 decimal max
            with pytest.raises(ValueError, match="invalid `precision` greater than max"):
                Quantity.from_str("1." + "0" * 10)  # 10 decimals > 9
        else:
            # On Linux/Mac with 16 decimal max
            with pytest.raises(ValueError, match="invalid `precision` greater than max"):
                Quantity.from_str("1." + "0" * 17)  # 17 decimals > 16

    def test_from_str_precision_preservation(self):

        # Whole numbers should have precision 0
        assert Quantity.from_str("100").precision == 0
        assert Quantity.from_str("1000000").precision == 0

        # Decimal places should determine precision
        assert Quantity.from_str("100.0").precision == 1
        assert Quantity.from_str("100.00").precision == 2
        assert Quantity.from_str("100.12345").precision == 5

        # Scientific notation with decimal results
        qty = Quantity.from_str("1.23e-2")
        assert str(qty) == "0.01"
        assert qty.precision == 2

        # Underscores shouldn't affect precision
        assert Quantity.from_str("1_000.123").precision == 3
        assert Quantity.from_str("1_000").precision == 0

    @pytest.mark.parametrize(
        ("input_val", "expected"),
        [
            # Test that values are preserved exactly
            ("1.115", "1.115"),
            ("1.125", "1.125"),
            ("1.135", "1.135"),
            ("1.145", "1.145"),
            # High precision values preserved up to FIXED_PRECISION
            (
                "0.9999999999999999" if FIXED_PRECISION > 9 else "0.999999999",
                "0.9999999999999999" if FIXED_PRECISION > 9 else "0.999999999",
            ),
            (
                "1.0000000000000001" if FIXED_PRECISION > 9 else "1.000000001",
                "1.0000000000000001" if FIXED_PRECISION > 9 else "1.000000001",
            ),
        ],
    )
    def test_from_str_rounding_behavior(self, input_val, expected):
        qty = Quantity.from_str(input_val)
        assert str(qty) == expected

    def test_from_str_boundary_values(self):
        # Test values near the boundaries of the Quantity type

        # Maximum value (should work)
        # Test reasonable large values
        qty_large = Quantity.from_str("1000000000")
        assert str(qty_large) == "1000000000"

        # Zero (should work)
        qty_zero = Quantity.from_str("0")
        assert qty_zero.as_double() == 0
        assert str(qty_zero) == "0"

        # Negative values should raise errors (Quantity is unsigned)
        with pytest.raises(ValueError):
            Quantity.from_str("-1")

        with pytest.raises(ValueError):
            Quantity.from_str("-0.001")

    @pytest.mark.parametrize(
        ("value", "expected"),
        [
            ["0", "0"],
            ["10.05", "10.05"],
            ["1000", "1_000"],
            ["1112", "1_112"],
            ["120100", "120_100"],
            ["200000", "200_000"],
            ["1000000", "1_000_000"],
            ["2500000", "2_500_000"],
            ["1111111", "1_111_111"],
            ["2523000", "2_523_000"],
            ["100000000", "100_000_000"],
        ],
    )
    def test_str_and_to_str(self, value, expected):
        # Arrange, Act, Assert
        assert Quantity.from_str(value).to_formatted_str() == expected

    def test_str_repr(self):
        # Arrange
        quantity = Quantity(2100.1666666, 6)

        # Act, Assert
        assert str(quantity) == "2100.166667"
        assert repr(quantity) == "Quantity(2100.166667)"

    def test_pickle_dumps_and_loads(self):
        # Arrange
        quantity = Quantity(1.2000, 2)

        # Act
        pickled = pickle.dumps(quantity)

        # Assert
        assert pickle.loads(pickled) == quantity  # noqa: S301 (testing pickle)

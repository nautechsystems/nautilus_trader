# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

import pickle
from decimal import Decimal

import pytest

from nautilus_trader.model.objects import Quantity


class TestQuantity:
    def test_instantiate_with_none_value_raises_type_error(self):
        # Arrange, Act, Assert
        with pytest.raises(TypeError):
            Quantity(None)

    def test_instantiate_with_negative_precision_raises_overflow_error(self):
        # Arrange, Act, Assert
        with pytest.raises(OverflowError):
            Quantity(1.0, precision=-1)

    def test_instantiate_with_precision_over_maximum_raises_value_error(self):
        # Arrange, Act, Assert
        with pytest.raises(ValueError):
            Quantity(1.0, precision=10)

    def test_instantiate_with_value_exceeding_limit_raises_value_error(self):
        # Arrange, Act, Assert
        with pytest.raises(ValueError):
            Quantity(18_446_744_073 + 1, precision=0)

    def test_instantiate_base_decimal_from_int(self):
        # Arrange, Act
        Quantity(1, precision=1)

    def test_instantiate_base_decimal_from_float(self):
        # Arrange, Act
        result = Quantity(1.12300, precision=5)

        # Assert
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
        assert type(result) == expected_type
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
        assert type(result) == expected_type
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
        assert result == "Quantity('1.1')"

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
        # Arrange, Act
        qty1 = Quantity.from_raw(1000000000000, 3)
        qty2 = Quantity(1000, 3)

        # Assert
        assert qty1 == qty2
        assert str(qty1) == "1000.000"
        assert qty1.precision == 3

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
        assert Quantity.from_str(value).to_str() == expected

    def test_str_repr(self):
        # Arrange
        quantity = Quantity(2100.1666666, 6)

        # Act, Assert
        assert str(quantity) == "2100.166667"
        assert repr(quantity) == "Quantity('2100.166667')"

    def test_pickle_dumps_and_loads(self):
        # Arrange
        quantity = Quantity(1.2000, 2)

        # Act
        pickled = pickle.dumps(quantity)

        # Assert
        assert pickle.loads(pickled) == quantity  # noqa (testing pickle)

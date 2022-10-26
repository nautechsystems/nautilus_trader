# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.model.currencies import AUD
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import AccountBalance
from nautilus_trader.model.objects import MarginBalance
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
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

    def test_instantiate_with_value_exceeding_positive_limit_raises_value_error(self):
        # Arrange, Act, Assert
        with pytest.raises(ValueError):
            Price(9_223_372_036 + 1, precision=0)

    def test_instantiate_with_value_exceeding_negative_limit_raises_value_error(self):
        # Arrange, Act, Assert
        with pytest.raises(ValueError):
            Price(-9_223_372_036 - 1, precision=0)

    def test_instantiate_base_decimal_from_int(self):
        # Arrange, Act
        result = Quantity(1, precision=1)

        # Assert
        assert str(result) == "1.0"

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
        "value, precision, expected",
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
        "value, expected",
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
        "value, expected",
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
        "value, expected",
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
        "value, precision, expected",
        [
            [0.0, 0, Quantity(0, precision=0)],
            [1.0, 0, Quantity(1, precision=0)],
            [1.123, 3, Quantity(1.123, precision=3)],
            [1.155, 2, Quantity(1.16, precision=2)],
        ],
    )
    def test_instantiate_with_various_precisions_returns_expected_decimal(
        self, value, precision, expected
    ):
        # Arrange, Act
        decimal_object = Quantity(value, precision)

        # Assert
        assert decimal_object == expected
        assert decimal_object.precision == precision

    @pytest.mark.parametrize(
        "value1, value2, expected",
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
        "value1, value2, expected",
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
        "value1, value2, expected",
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
        "value1, value2, expected1, expected2, expected3, expected4",
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
        "value1, value2, expected_type, expected_value",
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
        "value1, value2, expected_type, expected_value",
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
        "value1, value2, expected_type, expected_value",
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
        "value1, value2, expected_type, expected_value",
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
        assert expected_type == type(result)
        assert expected_value == result

    @pytest.mark.parametrize(
        "value1, value2, expected_type, expected_value",
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
        assert expected_type == type(result)
        assert expected_value == result

    @pytest.mark.parametrize(
        "value1, value2, expected_type, expected_value",
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
        result = value1 % value2  # noqa (not modulo formatting)

        # Assert
        assert expected_type == type(result)
        assert expected_value == result

    @pytest.mark.parametrize(
        "value1, value2, expected",
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
        assert expected == result

    @pytest.mark.parametrize(
        "value1, value2, expected",
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
        "value, expected",
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
        "value, precision, expected",
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
        assert "Quantity('1.1')" == result

    @pytest.mark.parametrize(
        "value, precision, expected",
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
        "value, expected",
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
        qty = Quantity.from_int(1000)

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
        "value, expected",
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
        assert "2100.166667" == str(quantity)
        assert "Quantity('2100.166667')" == repr(quantity)

    def test_pickle_dumps_and_loads(self):
        # Arrange
        quantity = Quantity(1.2000, 2)

        # Act
        pickled = pickle.dumps(quantity)

        # Assert
        assert pickle.loads(pickled) == quantity  # noqa (testing pickle)


class TestPrice:
    def test_instantiate_with_none_value_raises_type_error(self):
        # Arrange, Act, Assert
        with pytest.raises(TypeError):
            Price(None)

    def test_instantiate_with_negative_precision_raises_overflow_error(self):
        # Arrange, Act, Assert
        with pytest.raises(OverflowError):
            Price(1.0, precision=-1)

    def test_instantiate_with_precision_over_maximum_raises_overflow_error(self):
        # Arrange, Act, Assert
        with pytest.raises(ValueError):
            Price(1.0, precision=10)

    def test_instantiate_with_value_exceeding_positive_limit_raises_value_error(self):
        # Arrange, Act, Assert
        with pytest.raises(ValueError):
            Price(9_223_372_036 + 1, precision=0)

    def test_instantiate_with_value_exceeding_negative_limit_raises_value_error(self):
        # Arrange, Act, Assert
        with pytest.raises(ValueError):
            Price(-9_223_372_036 - 1, precision=0)

    def test_instantiate_base_decimal_from_int(self):
        # Arrange, Act
        result = Price(1, precision=1)

        # Assert
        assert str(result) == "1.0"

    def test_instantiate_base_decimal_from_float(self):
        # Arrange, Act
        result = Price(1.12300, precision=5)

        # Assert
        assert str(result) == "1.12300"

    def test_instantiate_base_decimal_from_decimal(self):
        # Arrange, Act
        result = Price(Decimal("1.23"), precision=1)

        # Assert
        assert str(result) == "1.2"

    def test_instantiate_base_decimal_from_str(self):
        # Arrange, Act
        result = Price.from_str("1.23")

        # Assert
        assert str(result) == "1.23"

    @pytest.mark.parametrize(
        "value, precision, expected",
        [
            [Price(2.15, precision=2), 0, Decimal("2")],
            [Price(2.15, precision=2), 1, Decimal("2.2")],
            [Price(2.255, precision=3), 2, Decimal("2.26")],
        ],
    )
    def test_round_with_various_digits_returns_expected_decimal(self, value, precision, expected):
        # Arrange, Act
        result = round(value, precision)

        # Assert
        assert result == expected

    @pytest.mark.parametrize(
        "value, expected",
        [
            [Price(-0, precision=0), Decimal("0")],
            [Price(0, precision=0), Decimal("0")],
            [Price(1, precision=0), Decimal("1")],
            [Price(-1, precision=0), Decimal("1")],
            [Price(-1.1, precision=1), Decimal("1.1")],
        ],
    )
    def test_abs_with_various_values_returns_expected_decimal(self, value, expected):
        # Arrange, Act
        result = abs(value)

        # Assert
        assert result == expected

    @pytest.mark.parametrize(
        "value, expected",
        [
            [
                Price(-1, precision=0),
                Decimal("-1"),
            ],  # Matches built-in decimal.Decimal behaviour
            [Price(0, 0), Decimal("0")],
        ],
    )
    def test_pos_with_various_values_returns_expected_decimal(self, value, expected):
        # Arrange, Act
        result = +value

        # Assert
        assert result == expected

    @pytest.mark.parametrize(
        "value, expected",
        [
            [Price(1, precision=0), Decimal("-1")],
            [Price(0, precision=0), Decimal("0")],
        ],
    )
    def test_neg_with_various_values_returns_expected_decimal(self, value, expected):
        # Arrange, Act
        result = -value

        # Assert
        assert result == expected

    @pytest.mark.parametrize(
        "value, expected",
        [
            [0, Price(0, precision=0)],
            [1, Price(1, precision=0)],
            [-1, Price(-1, precision=0)],
            [Decimal(0), Price(0, precision=0)],
            [Decimal("1.1"), Price(1.1, precision=1)],
            [Decimal("-1.1"), Price(-1.1, precision=1)],
            [Price(0, precision=0), Price(0, precision=0)],
            [Price(1.1, precision=1), Price(1.1, precision=1)],
            [Price(-1.1, precision=1), Price(-1.1, precision=1)],
        ],
    )
    def test_instantiate_with_various_valid_inputs_returns_expected_decimal(self, value, expected):
        # Arrange, Act
        decimal_object = Price(value, 2)

        # Assert
        assert decimal_object == expected

    @pytest.mark.parametrize(
        "value, precision, expected",
        [
            [0.0, 0, Price(0, precision=0)],
            [1.0, 0, Price(1, precision=0)],
            [-1.0, 0, Price(-1, precision=0)],
            [1.123, 3, Price(1.123, precision=3)],
            [-1.123, 3, Price(-1.123, precision=3)],
            [1.155, 2, Price(1.16, precision=2)],
        ],
    )
    def test_instantiate_with_various_precisions_returns_expected_decimal(
        self, value, precision, expected
    ):
        # Arrange, Act
        decimal_object = Price(value, precision)

        # Assert
        assert decimal_object == expected
        assert decimal_object.precision == precision

    @pytest.mark.parametrize(
        "value1, value2, expected",
        [
            [0, -0, True],
            [-0, 0, True],
            [-1, -1, True],
            [1, 1, True],
            [1.1, 1.1, True],
            [-1.1, -1.1, True],
            [0, 1, False],
            [-1, 0, False],
            [-1, -2, False],
            [1, 2, False],
            [1.1, 1.12, False],
            [-1.12, -1.1, False],
        ],
    )
    def test_equality_with_various_values_returns_expected_result(self, value1, value2, expected):
        # Arrange, Act
        result = Price(value1, 2) == Price(value2, 2)

        # Assert
        assert result == expected

    @pytest.mark.parametrize(
        "value1, value2, expected",
        [
            [0, -0, True],
            [-0, 0, True],
            [-1, -1, True],
            [1, 1, True],
            [0, 1, False],
            [-1, 0, False],
            [-1, -2, False],
            [1, 2, False],
        ],
    )
    def test_equality_with_various_int_returns_expected_result(self, value1, value2, expected):
        # Arrange, Act
        result1 = Price(value1, 0) == value2
        result2 = value2 == Price(value1, 0)

        # Assert
        assert result1 == expected
        assert result2 == expected

    @pytest.mark.parametrize(
        "value1, value2, expected",
        [
            [Price(0, precision=0), Decimal(0), True],
            [Price(0, precision=0), Decimal(-0), True],
            [Price(1, precision=0), Decimal(0), False],
        ],
    )
    def test_equality_with_various_decimals_returns_expected_result(self, value1, value2, expected):
        # Arrange, Act
        result = value1 == value2

        # Assert
        assert result == expected

    @pytest.mark.parametrize(
        "value1, value2, expected1, expected2, expected3, expected4",
        [
            [0, 0, False, True, True, False],
            [1, 0, True, True, False, False],
            [-1, 0, False, False, True, True],
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
        assert (Price(value1, precision=0) > Price(value2, precision=0)) == expected1
        assert (Price(value1, precision=0) >= Price(value2, precision=0)) == expected2
        assert (Price(value1, precision=0) <= Price(value2, precision=0)) == expected3
        assert (Price(value1, precision=0) < Price(value2, precision=0)) == expected4

    @pytest.mark.parametrize(
        "value1, value2, expected_type, expected_value",
        [
            [Price(0, precision=0), Price(0, precision=0), Decimal, 0],
            [
                Price(0, precision=0),
                Price(1.1, precision=1),
                Decimal,
                Decimal("1.1"),
            ],
            [Price(0, precision=0), 0, Decimal, 0],
            [Price(0, precision=0), 1, Decimal, 1],
            [0, Price(0, precision=0), Decimal, 0],
            [1, Price(0, precision=0), Decimal, 1],
            [Price(0, precision=0), 0.1, float, 0.1],
            [Price(0, precision=0), 1.1, float, 1.1],
            [-1.1, Price(0, precision=0), float, -1.1],
            [1.1, Price(0, precision=0), float, 1.1],
            [
                Price(1, precision=0),
                Price(1.1, precision=1),
                Decimal,
                Decimal("2.1"),
            ],
            [Price(1, precision=0), Decimal("1.1"), Decimal, Decimal("2.1")],
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
        "value1, value2, expected_type, expected_value",
        [
            [Price(0, precision=0), Price(0, precision=0), Decimal, 0],
            [
                Price(0, precision=0),
                Price(1.1, precision=1),
                Decimal,
                Decimal("-1.1"),
            ],
            [Price(0, precision=0), 0, Decimal, 0],
            [Price(0, precision=0), 1, Decimal, -1],
            [0, Price(0, precision=0), Decimal, 0],
            [1, Price(1, precision=0), Decimal, 0],
            [Price(0, precision=0), 0.1, float, -0.1],
            [Price(0, precision=0), 1.1, float, -1.1],
            [0.1, Price(1, precision=0), float, -0.9],
            [1.1, Price(1, precision=0), float, 0.10000000000000009],
            [
                Price(1, precision=0),
                Price(1.1, precision=1),
                Decimal,
                Decimal("-0.1"),
            ],
            [Price(1, precision=0), Decimal("1.1"), Decimal, Decimal("-0.1")],
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
        "value1, value2, expected_type, expected_value",
        [
            [Price(0, 0), 0, Decimal, 0],
            [Price(1, 0), 1, Decimal, 1],
            [1, Price(1, 0), Decimal, 1],
            [2, Price(3, 0), Decimal, 6],
            [Price(2, 0), 1.0, float, 2],
            [1.1, Price(2, 0), float, 2.2],
            [Price(1.1, 1), Price(1.1, 1), Decimal, Decimal("1.21")],
            [Price(1.1, 1), Decimal("1.1"), Decimal, Decimal("1.21")],
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
        "value1, value2, expected_type, expected_value",
        [
            [1, Price(1, 0), Decimal, 1],
            [1.1, Price(1.1, 1), float, 1],
            [Price(0, 0), 1, Decimal, 0],
            [Price(1, 0), 2, Decimal, Decimal("0.5")],
            [2, Price(1, 0), Decimal, Decimal("2.0")],
            [Price(2, 0), 1.1, float, 1.8181818181818181],
            [1.1, Price(2, 0), float, 1.1 / 2],
            [
                Price(1.1, 1),
                Price(1.2, 1),
                Decimal,
                Decimal("0.9166666666666666666666666667"),
            ],
            [
                Price(1.1, 1),
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
        assert expected_type == type(result)
        assert expected_value == result

    @pytest.mark.parametrize(
        "value1, value2, expected_type, expected_value",
        [
            [1, Price(1, 0), Decimal, 1],
            [Price(0, 0), 1, Decimal, 0],
            [Price(1, 0), 2, Decimal, Decimal(0)],
            [2, Price(1, 0), Decimal, Decimal(2)],
            [2.1, Price(1.1, 1), float, 1],
            [4.4, Price(1.1, 1), float, 4],
            [Price(2.1, 1), 1.1, float, 1],
            [Price(4.4, 1), 1.1, float, 4],
            [Price(1.1, 1), Price(1.2, 1), Decimal, Decimal(0)],
            [Price(1.1, 1), Decimal("1.2"), Decimal, Decimal(0)],
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
        assert expected_type == type(result)
        assert expected_value == result

    @pytest.mark.parametrize(
        "value1, value2, expected_type, expected_value",
        [
            [1, Price(1, 0), Decimal, 1],
            [Price(100, 0), 10, Decimal, 0],
            [Price(23, 0), 2, Decimal, 1],
            [2.1, Price(1.1, 1), float, 1.0],
            [1.1, Price(2.1, 1), float, 1.1],
            [Price(2.1, 1), 1.1, float, 1.0],
            [Price(1.1, 1), 2.1, float, 1.1],
            [Price(1.1, 1), Price(0.2, 1), Decimal, Decimal("0.1")],
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
        result = value1 % value2  # noqa (not modulo formatting)

        # Assert
        assert expected_type == type(result)
        assert expected_value == result

    @pytest.mark.parametrize(
        "value1, value2, expected",
        [
            [Price(1, 0), Price(2, 0), Price(2, 0)],
            [Price(1, 0), 2, 2],
            [Price(1, 0), Decimal(2), Decimal(2)],
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
        assert expected == result

    @pytest.mark.parametrize(
        "value1, value2, expected",
        [
            [Price(1, 0), Price(2, 0), Price(1, 0)],
            [Price(1, 0), 2, Price(1, 0)],
            [Price(2, 0), Decimal(1), Decimal(1)],
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
        "value, expected",
        [["1", 1], ["1.1", 1]],
    )
    def test_int(self, value, expected):
        # Arrange
        decimal1 = Price.from_str(value)

        # Act, Assert
        assert int(decimal1) == expected

    def test_hash(self):
        # Arrange
        decimal1 = Price(1.1, 1)
        decimal2 = Price(1.1, 1)

        # Act, Assert
        assert isinstance(hash(decimal2), int)
        assert hash(decimal1) == hash(decimal2)

    @pytest.mark.parametrize(
        "value, precision, expected",
        [
            [0, 0, "0"],
            [-0, 0, "0"],
            [-1, 0, "-1"],
            [1, 0, "1"],
            [1.1, 1, "1.1"],
            [-1.1, 1, "-1.1"],
        ],
    )
    def test_str_with_various_values_returns_expected_string(
        self,
        value,
        precision,
        expected,
    ):
        # Arrange, Act
        decimal_object = Price(value, precision=precision)

        # Assert
        assert str(decimal_object) == expected

    def test_repr(self):
        # Arrange, Act
        result = repr(Price(1.1, 1))

        # Assert
        assert "Price('1.1')" == result

    @pytest.mark.parametrize(
        "value, precision, expected",
        [
            [0, 0, Price(0, 0)],
            [-0, 0, Price(0, 0)],
            [-1, 0, Price(-1, 0)],
            [1, 0, Price(1, 0)],
            [1.1, 1, Price(1.1, 1)],
            [-1.1, 1, Price(-1.1, 1)],
        ],
    )
    def test_as_decimal_with_various_values_returns_expected_value(
        self,
        value,
        precision,
        expected,
    ):
        # Arrange, Act
        result = Price(value, precision=precision)

        # Assert
        assert result == expected

    @pytest.mark.parametrize(
        "value, expected",
        [[0, 0], [-0, 0], [-1, -1], [1, 1], [1.1, 1.1], [-1.1, -1.1]],
    )
    def test_as_double_with_various_values_returns_expected_value(self, value, expected):
        # Arrange, Act
        result = Price(value, 1).as_double()

        # Assert
        assert result == expected

    def test_calling_new_returns_an_expected_zero_price(self):
        # Arrange, Act
        new_price = Price.__new__(Price, 1, 1)

        # Assert
        assert new_price == 0

    def test_from_raw_returns_expected_price(self):
        # Arrange, Act
        price1 = Price.from_raw(1000000000000, 3)
        price2 = Price(1000, 3)

        # Assert
        assert price1 == price2
        assert str(price1) == "1000.000"
        assert price1.precision == 3

    def test_equality(self):
        # Arrange, Act
        price1 = Price(1.0, precision=1)
        price2 = Price(1.5, precision=1)

        # Assert
        assert price1 == price1
        assert price1 != price2
        assert price2 > price1

    def test_from_int_returns_expected_value(self):
        # Arrange, Act
        price = Price.from_int(100)

        # Assert
        assert str(price) == "100"
        assert price.precision == 0

    @pytest.mark.parametrize(
        "value, string, precision",
        [
            ["100.11", "100.11", 2],
            ["1E7", "10000000", 0],
            ["1E-7", "0.0000001", 7],
            ["1e-2", "0.01", 2],
        ],
    )
    def test_from_str_returns_expected_value(self, value, string, precision):
        # Arrange, Act
        price = Price.from_str(value)

        # Assert
        assert str(price) == string
        assert price.precision == precision

    def test_str_repr(self):
        # Arrange, Act
        price = Price(1.00000, precision=5)

        # Assert
        assert "1.00000" == str(price)
        assert "Price('1.00000')" == repr(price)

    def test_pickle_dumps_and_loads(self):
        # Arrange
        price = Price(1.2000, 2)

        # Act
        pickled = pickle.dumps(price)

        # Assert
        assert pickle.loads(pickled) == price  # noqa (testing pickle)


class TestMoney:
    def test_instantiate_with_none_currency_raises_type_error(self):
        # Arrange, Act, Assert
        with pytest.raises(TypeError):
            Money(1.0, None)

    def test_instantiate_with_value_exceeding_positive_limit_raises_value_error(self):
        # Arrange, Act, Assert
        with pytest.raises(ValueError):
            Money(9_223_372_036 + 1, currency=USD)

    def test_instantiate_with_value_exceeding_negative_limit_raises_value_error(self):
        # Arrange, Act, Assert
        with pytest.raises(ValueError):
            Money(-9_223_372_036 - 1, currency=USD)

    def test_instantiate_with_none_value_returns_money_with_zero_amount(self):
        # Arrange, Act
        money_zero = Money(None, currency=USD)

        # Assert
        assert 0 == money_zero.as_decimal()

    @pytest.mark.parametrize(
        "value, expected",
        [
            [0, Money(0, USD)],
            [1, Money(1, USD)],
            [-1, Money(-1, USD)],
            ["0", Money(0, USD)],
            ["0.0", Money(0, USD)],
            ["-0.0", Money(0, USD)],
            ["1.0", Money(1, USD)],
            ["-1.0", Money(-1, USD)],
            [Decimal(0), Money(0, USD)],
            [Decimal("1.1"), Money(1.1, USD)],
            [Decimal("-1.1"), Money(-1.1, USD)],
        ],
    )
    def test_instantiate_with_various_valid_inputs_returns_expected_money(self, value, expected):
        # Arrange, Act
        money = Money(value, USD)

        # Assert
        assert money == expected

    def test_pickling(self):
        # Arrange
        money = Money(1, USD)

        # Act
        pickled = pickle.dumps(money)
        unpickled = pickle.loads(pickled)  # noqa S301 (pickle is safe here)

        # Assert
        assert unpickled == money

    def test_as_double_returns_expected_result(self):
        # Arrange, Act
        money = Money(1, USD)

        # Assert
        assert 1.0 == money.as_double()
        assert "1.00" == str(money)

    def test_initialized_with_many_decimals_rounds_to_currency_precision(self):
        # Arrange, Act
        result1 = Money(1000.333, USD)
        result2 = Money(5005.556666, USD)

        # Assert
        assert "1_000.33 USD" == result1.to_str()
        assert "5_005.56 USD" == result2.to_str()

    def test_hash(self):
        # Arrange
        money0 = Money(0, USD)

        # Act, Assert
        assert isinstance(hash(money0), int)
        assert hash(money0) == hash(money0)

    def test_str(self):
        # Arrange
        money0 = Money(0, USD)
        money1 = Money(1, USD)
        money2 = Money(1_000_000, USD)

        # Act, Assert
        assert "0.00" == str(money0)
        assert "1.00" == str(money1)
        assert "1000000.00" == str(money2)
        assert "1_000_000.00 USD" == money2.to_str()

    def test_repr(self):
        # Arrange
        money = Money(1.00, USD)

        # Act
        result = repr(money)

        # Assert
        assert "Money('1.00', USD)" == result

    def test_from_str_when_malformed_raises_value_error(self):
        # Arrange
        value = "@"

        # Act, Assert
        with pytest.raises(ValueError):
            Money.from_str(value)

    @pytest.mark.parametrize(
        "value, expected",
        [
            ["1.00 USD", Money(1.00, USD)],
            ["1.001 AUD", Money(1.00, AUD)],
        ],
    )
    def test_from_str_given_valid_strings_returns_expected_result(
        self,
        value,
        expected,
    ):
        # Arrange, Act
        result = Money.from_str(value)

        # Assert
        assert result == expected


class TestAccountBalance:
    def test_instantiate_str_repr(self):
        # Arrange, Act
        balance = AccountBalance(
            total=Money(1_525_000, USD),
            locked=Money(25_000, USD),
            free=Money(1_500_000, USD),
        )

        # Assert
        assert (
            str(balance)
            == "AccountBalance(total=1_525_000.00 USD, locked=25_000.00 USD, free=1_500_000.00 USD)"
        )
        assert (
            repr(balance)
            == "AccountBalance(total=1_525_000.00 USD, locked=25_000.00 USD, free=1_500_000.00 USD)"
        )


class TestMarginBalance:
    def test_instantiate_str_repr_with_instrument_id(self):
        # Arrange, Act
        margin = MarginBalance(
            initial=Money(5_000, USD),
            maintenance=Money(25_000, USD),
            instrument_id=InstrumentId(Symbol("AUD/USD"), Venue("IDEALPRO")),
        )

        # Assert
        assert (
            str(margin)
            == "MarginBalance(initial=5_000.00 USD, maintenance=25_000.00 USD, instrument_id=AUD/USD.IDEALPRO)"
        )
        assert (
            repr(margin)
            == "MarginBalance(initial=5_000.00 USD, maintenance=25_000.00 USD, instrument_id=AUD/USD.IDEALPRO)"
        )

    def test_instantiate_str_repr_without_instrument_id(self):
        # Arrange, Act
        margin = MarginBalance(
            initial=Money(5_000, USD),
            maintenance=Money(25_000, USD),
        )

        # Assert
        assert (
            str(margin)
            == "MarginBalance(initial=5_000.00 USD, maintenance=25_000.00 USD, instrument_id=None)"
        )
        assert (
            repr(margin)
            == "MarginBalance(initial=5_000.00 USD, maintenance=25_000.00 USD, instrument_id=None)"
        )

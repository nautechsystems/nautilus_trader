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

from nautilus_trader.core.nautilus_pyo3 import Price
from nautilus_trader.model import convert_to_raw_int
from nautilus_trader.model.objects import FIXED_PRECISION
from nautilus_trader.model.objects import PRICE_MAX
from nautilus_trader.model.objects import PRICE_MIN


class TestPrice:
    def test_instantiate_with_nan_raises_value_error(self):
        # Arrange, Act, Assert
        with pytest.raises(ValueError):
            Price(math.nan, precision=0)

    def test_instantiate_with_none_value_raises_type_error(self):
        # Arrange, Act, Assert
        with pytest.raises(TypeError):
            Price(None, precision=0)

    def test_instantiate_with_negative_precision_raises_overflow_error(self):
        # Arrange, Act, Assert
        with pytest.raises(OverflowError):
            Price(1.0, precision=-1)

    def test_instantiate_with_precision_over_maximum_raises_overflow_error(self):
        # Arrange, Act, Assert
        with pytest.raises(ValueError):
            Price(1.0, precision=FIXED_PRECISION + 1)

    def test_instantiate_with_value_exceeding_positive_limit_raises_value_error(self):
        # Arrange, Act, Assert
        with pytest.raises(ValueError):
            Price(PRICE_MAX + 1, precision=0)

    def test_instantiate_with_value_exceeding_negative_limit_raises_value_error(self):
        # Arrange, Act, Assert
        with pytest.raises(ValueError):
            Price(PRICE_MIN - 1, precision=0)

    def test_instantiate_base_decimal_from_int(self):
        # Arrange, Act
        result = Price(1, precision=1)

        # Assert
        assert result.raw == 10**FIXED_PRECISION
        assert str(result) == "1.0"

    def test_instantiate_base_decimal_from_float(self):
        # Arrange, Act
        result = Price(1.12300, precision=5)

        # Assert
        expected_raw = int(1.123 * (10**FIXED_PRECISION))
        assert result.raw == expected_raw
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
        ("value", "precision", "expected"),
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
        ("value", "expected"),
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
        ("value", "expected"),
        [
            [
                Price(-1.0, precision=0),
                Decimal("1"),
            ],  # Matches built-in decimal.Decimal behavior
            [
                Price(0, 0),
                Decimal("0"),
            ],
        ],
    )
    def test_pos_with_various_values_returns_expected_decimal(self, value, expected):
        # Arrange, Act
        result = +value

        # Assert
        assert result == expected

    @pytest.mark.parametrize(
        ("value", "expected"),
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
        ("value", "expected"),
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
        ("value", "precision", "expected"),
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
        self,
        value,
        precision,
        expected,
    ):
        # Arrange, Act
        decimal_object = Price(value, precision)

        # Assert
        assert decimal_object == expected
        assert decimal_object.precision == precision

    @pytest.mark.parametrize(
        ("value1", "value2", "expected"),
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
        ("value1", "value2", "expected"),
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
        ("value1", "value2", "expected"),
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
        ("value1", "value2", "expected1", "expected2", "expected3", "expected4"),
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
        ("value1", "value2", "expected_type", "expected_value"),
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
        ("value1", "value2", "expected_type", "expected_value"),
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
        ("value1", "value2", "expected_type", "expected_value"),
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
        ("value1", "value2", "expected_type", "expected_value"),
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
        assert isinstance(result, expected_type)
        assert result == expected_value

    @pytest.mark.parametrize(
        ("value1", "value2", "expected_type", "expected_value"),
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
        assert type(result) is expected_type
        assert result == expected_value

    @pytest.mark.parametrize(
        ("value1", "value2", "expected_type", "expected_value"),
        [
            [1, Price(1, 0), Decimal, 0],
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
        result = value1 % value2

        # Assert
        assert type(result) is expected_type
        assert result == expected_value

    @pytest.mark.parametrize(
        ("value1", "value2", "expected"),
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
        assert result == expected

    @pytest.mark.parametrize(
        ("value1", "value2", "expected"),
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
        ("value", "expected"),
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
        ("value", "precision", "expected"),
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
        assert result == "Price(1.1)"

    @pytest.mark.parametrize(
        ("value", "precision", "expected"),
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
        ("value", "expected"),
        [[0, 0], [-0, 0], [-1, -1], [1, 1], [1.1, 1.1], [-1.1, -1.1]],
    )
    def test_as_double_with_various_values_returns_expected_value(self, value, expected):
        # Arrange, Act
        result = Price(value, 1).as_double()

        # Assert
        assert result == expected

    def test_from_raw_returns_expected_price(self):
        # Arrange
        value = 1000
        precision = 3

        # Act
        raw_value = convert_to_raw_int(value, precision)
        price1 = Price.from_raw(raw_value, precision)
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
        ("value", "string", "precision"),
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
        assert str(price) == "1.00000"
        assert repr(price) == "Price(1.00000)"

    def test_pickle_dumps_and_loads(self):
        # Arrange
        price = Price(1.2000, 2)

        # Act
        pickled = pickle.dumps(price)

        # Assert
        assert pickle.loads(pickled) == price  # noqa: S301 (testing pickle)

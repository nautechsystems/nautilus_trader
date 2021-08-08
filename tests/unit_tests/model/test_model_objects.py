# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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

from decimal import Decimal

import pytest

from nautilus_trader.model.currencies import AUD
from nautilus_trader.model.currencies import BTC
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.objects import BaseDecimal
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity


class TestBaseDecimal:
    def test_instantiate_with_none_value_raises_type_error(self):
        # Arrange
        # Act
        # Assert
        with pytest.raises(TypeError):
            BaseDecimal(None)

    def test_instantiate_with_negative_precision_raises_overflow_error(self):
        # Arrange
        # Act
        # Assert
        with pytest.raises(OverflowError):
            BaseDecimal(1.11, precision=-1)

    def test_instantiate_base_decimal_from_int(self):
        # Arrange, Act
        result = BaseDecimal(1, precision=1)

        # Assert
        assert str(result) == "1.0"

    def test_instantiate_base_decimal_from_float(self):
        # Arrange, Act
        result = BaseDecimal(1.12300, precision=5)

        # Assert
        assert str(result) == "1.12300"

    def test_instantiate_base_decimal_from_decimal(self):
        # Arrange, Act
        result = BaseDecimal(Decimal("1.23"), precision=1)

        # Assert
        assert str(result) == "1.2"

    def test_instantiate_base_decimal_from_str(self):
        # Arrange, Act
        result = BaseDecimal("1.23", precision=1)

        # Assert
        assert str(result) == "1.2"

    @pytest.mark.parametrize(
        "value, precision, expected",
        [
            [BaseDecimal(2.15, precision=2), 0, Decimal("2")],
            [BaseDecimal(2.15, precision=2), 1, Decimal("2.2")],
            [BaseDecimal(2.255, precision=3), 2, Decimal("2.26")],
        ],
    )
    def test_round_with_various_digits_returns_expected_decimal(self, value, precision, expected):
        # Arrange
        # Act
        result = round(value, precision)

        # Assert
        assert result == expected

    @pytest.mark.parametrize(
        "value, expected",
        [
            [BaseDecimal(-0, precision=0), Decimal("0")],
            [BaseDecimal(0, precision=0), Decimal("0")],
            [BaseDecimal(1, precision=0), Decimal("1")],
            [BaseDecimal(-1, precision=0), Decimal("1")],
            [BaseDecimal(-1.1, precision=1), Decimal("1.1")],
        ],
    )
    def test_abs_with_various_values_returns_expected_decimal(self, value, expected):
        # Arrange
        # Act
        result = abs(value)

        # Assert
        assert result == expected

    @pytest.mark.parametrize(
        "value, expected",
        [
            [
                BaseDecimal(-1, precision=0),
                Decimal("-1"),
            ],  # Matches built-in decimal.Decimal behaviour
            [BaseDecimal(0, 0), Decimal("0")],
        ],
    )
    def test_pos_with_various_values_returns_expected_decimal(self, value, expected):
        # Arrange
        # Act
        result = +value

        # Assert
        assert result == expected

    @pytest.mark.parametrize(
        "value, expected",
        [
            [BaseDecimal(1, precision=0), Decimal("-1")],
            [BaseDecimal(0, precision=0), Decimal("0")],
        ],
    )
    def test_neg_with_various_values_returns_expected_decimal(self, value, expected):
        # Arrange
        # Act
        result = -value

        # Assert
        assert result == expected

    @pytest.mark.parametrize(
        "value, expected",
        [
            [0, BaseDecimal(0, precision=0)],
            [1, BaseDecimal(1, precision=0)],
            [-1, BaseDecimal(-1, precision=0)],
            [Decimal(), BaseDecimal(0, precision=0)],
            [Decimal("1.1"), BaseDecimal(1.1, precision=1)],
            [Decimal("-1.1"), BaseDecimal(-1.1, precision=1)],
            [BaseDecimal(0, precision=0), BaseDecimal(0, precision=0)],
            [BaseDecimal(1.1, precision=1), BaseDecimal(1.1, precision=1)],
            [BaseDecimal(-1.1, precision=1), BaseDecimal(-1.1, precision=1)],
        ],
    )
    def test_instantiate_with_various_valid_inputs_returns_expected_decimal(self, value, expected):
        # Arrange
        # Act
        decimal_object = BaseDecimal(value, 2)

        # Assert
        assert decimal_object == expected

    @pytest.mark.parametrize(
        "value, precision, expected",
        [
            [0.0, 0, BaseDecimal(0, precision=0)],
            [1.0, 0, BaseDecimal(1, precision=0)],
            [-1.0, 0, BaseDecimal(-1, precision=0)],
            [1.123, 3, BaseDecimal(1.123, precision=3)],
            [-1.123, 3, BaseDecimal(-1.123, precision=3)],
            [1.155, 2, BaseDecimal(1.16, precision=2)],
        ],
    )
    def test_instantiate_with_various_precisions_returns_expected_decimal(
        self, value, precision, expected
    ):
        # Arrange
        # Act
        decimal_object = BaseDecimal(value, precision)

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
        # Arrange
        # Act
        result = BaseDecimal(value1, 2) == BaseDecimal(value2, 2)

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
        # Arrange
        # Act
        result1 = BaseDecimal(value1, 0) == value2
        result2 = value2 == BaseDecimal(value1, 0)

        # Assert
        assert result1 == expected
        assert result2 == expected

    @pytest.mark.parametrize(
        "value1, value2, expected",
        [
            [BaseDecimal(0, precision=0), Decimal(), True],
            [BaseDecimal(0, precision=0), Decimal(-0), True],
            [BaseDecimal(1, precision=0), Decimal(), False],
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
        assert (BaseDecimal(value1, precision=0) > BaseDecimal(value2, precision=0)) == expected1
        assert (BaseDecimal(value1, precision=0) >= BaseDecimal(value2, precision=0)) == expected2
        assert (BaseDecimal(value1, precision=0) <= BaseDecimal(value2, precision=0)) == expected3
        assert (BaseDecimal(value1, precision=0) < BaseDecimal(value2, precision=0)) == expected4

    @pytest.mark.parametrize(
        "value1, value2, expected_type, expected_value",
        [
            [BaseDecimal(0, precision=0), BaseDecimal(0, precision=0), Decimal, 0],
            [
                BaseDecimal(0, precision=0),
                BaseDecimal(1.1, precision=1),
                Decimal,
                Decimal("1.1"),
            ],
            [BaseDecimal(0, precision=0), 0, Decimal, 0],
            [BaseDecimal(0, precision=0), 1, Decimal, 1],
            [0, BaseDecimal(0, precision=0), Decimal, 0],
            [1, BaseDecimal(0, precision=0), Decimal, 1],
            [BaseDecimal(0, precision=0), 0.1, float, 0.1],
            [BaseDecimal(0, precision=0), 1.1, float, 1.1],
            [-1.1, BaseDecimal(0, precision=0), float, -1.1],
            [1.1, BaseDecimal(0, precision=0), float, 1.1],
            [
                BaseDecimal(1, precision=0),
                BaseDecimal(1.1, precision=1),
                Decimal,
                Decimal("2.1"),
            ],
            [BaseDecimal(1, precision=0), Decimal("1.1"), Decimal, Decimal("2.1")],
        ],
    )
    def test_addition_with_various_types_returns_expected_result(
        self,
        value1,
        value2,
        expected_type,
        expected_value,
    ):
        # Arrange
        # Act
        result = value1 + value2

        # Assert
        assert isinstance(result, expected_type)
        assert result == expected_value

    @pytest.mark.parametrize(
        "value1, value2, expected_type, expected_value",
        [
            [BaseDecimal(0, precision=0), BaseDecimal(0, precision=0), Decimal, 0],
            [
                BaseDecimal(0, precision=0),
                BaseDecimal(1.1, precision=1),
                Decimal,
                Decimal("-1.1"),
            ],
            [BaseDecimal(0, precision=0), 0, Decimal, 0],
            [BaseDecimal(0, precision=0), 1, Decimal, -1],
            [0, BaseDecimal(0, precision=0), Decimal, 0],
            [1, BaseDecimal(1, precision=0), Decimal, 0],
            [BaseDecimal(0, precision=0), 0.1, float, -0.1],
            [BaseDecimal(0, precision=0), 1.1, float, -1.1],
            [0.1, BaseDecimal(1, precision=0), float, -0.9],
            [1.1, BaseDecimal(1, precision=0), float, 0.10000000000000009],
            [
                BaseDecimal(1, precision=0),
                BaseDecimal(1.1, precision=1),
                Decimal,
                Decimal("-0.1"),
            ],
            [BaseDecimal(1, precision=0), Decimal("1.1"), Decimal, Decimal("-0.1")],
        ],
    )
    def test_subtraction_with_various_types_returns_expected_result(
        self,
        value1,
        value2,
        expected_type,
        expected_value,
    ):
        # Arrange
        # Act
        result = value1 - value2

        # Assert
        assert isinstance(result, expected_type)
        assert result == expected_value

    @pytest.mark.parametrize(
        "value1, value2, expected_type, expected_value",
        [
            [BaseDecimal(0, 0), 0, Decimal, 0],
            [BaseDecimal(1, 0), 1, Decimal, 1],
            [1, BaseDecimal(1, 0), Decimal, 1],
            [2, BaseDecimal(3, 0), Decimal, 6],
            [BaseDecimal(2, 0), 1.0, float, 2],
            [1.1, BaseDecimal(2, 0), float, 2.2],
            [BaseDecimal(1.1, 1), BaseDecimal(1.1, 1), Decimal, Decimal("1.21")],
            [BaseDecimal(1.1, 1), Decimal("1.1"), Decimal, Decimal("1.21")],
        ],
    )
    def test_multiplication_with_various_types_returns_expected_result(
        self,
        value1,
        value2,
        expected_type,
        expected_value,
    ):
        # Arrange
        # Act
        result = value1 * value2

        # Assert
        assert isinstance(result, expected_type)
        assert result == expected_value

    @pytest.mark.parametrize(
        "value1, value2, expected_type, expected_value",
        [
            [1, BaseDecimal(1, 0), Decimal, 1],
            [1.1, BaseDecimal(1.1, 1), float, 1],
            [BaseDecimal(0, 0), 1, Decimal, 0],
            [BaseDecimal(1, 0), 2, Decimal, Decimal("0.5")],
            [2, BaseDecimal(1, 0), Decimal, Decimal("2.0")],
            [BaseDecimal(2, 0), 1.1, float, 1.8181818181818181],
            [1.1, BaseDecimal(2, 0), float, 1.1 / 2],
            [
                BaseDecimal(1.1, 1),
                BaseDecimal(1.2, 1),
                Decimal,
                Decimal("0.9166666666666666666666666667"),
            ],
            [
                BaseDecimal(1.1, 1),
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
        # Arrange
        # Act
        result = value1 / value2

        # Assert
        assert expected_type == type(result)
        assert expected_value == result

    @pytest.mark.parametrize(
        "value1, value2, expected_type, expected_value",
        [
            [1, BaseDecimal(1, 0), Decimal, 1],
            [BaseDecimal(0, 0), 1, Decimal, 0],
            [BaseDecimal(1, 0), 2, Decimal, Decimal(0)],
            [2, BaseDecimal(1, 0), Decimal, Decimal(2)],
            [2.1, BaseDecimal(1.1, 1), float, 1],
            [4.4, BaseDecimal(1.1, 1), float, 4],
            [BaseDecimal(2.1, 1), 1.1, float, 1],
            [BaseDecimal(4.4, 1), 1.1, float, 4],
            [BaseDecimal(1.1, 1), BaseDecimal(1.2, 1), Decimal, Decimal(0)],
            [BaseDecimal(1.1, 1), Decimal("1.2"), Decimal, Decimal(0)],
        ],
    )
    def test_floor_division_with_various_types_returns_expected_result(
        self,
        value1,
        value2,
        expected_type,
        expected_value,
    ):
        # Arrange
        # Act
        result = value1 // value2

        # Assert
        assert expected_type == type(result)
        assert expected_value == result

    @pytest.mark.parametrize(
        "value1, value2, expected_type, expected_value",
        [
            [1, BaseDecimal(1, 0), Decimal, 0],
            [BaseDecimal(100, 0), 10, Decimal, 0],
            [BaseDecimal(23, 0), 2, Decimal, 1],
            [2, BaseDecimal(1, 0), Decimal, 0],
            [2.1, BaseDecimal(1.1, 1), float, 1.0],
            [1.1, BaseDecimal(2.1, 1), float, 1.1],
            [BaseDecimal(2.1, 1), 1.1, float, 1.0],
            [BaseDecimal(1.1, 1), 2.1, float, 1.1],
            [BaseDecimal(1.1, 1), BaseDecimal(0.2, 1), Decimal, Decimal("0.1")],
            [BaseDecimal(1.1, 1), Decimal("0.2"), Decimal, Decimal("0.1")],
        ],
    )
    def test_mod_with_various_types_returns_expected_result(
        self,
        value1,
        value2,
        expected_type,
        expected_value,
    ):
        # Arrange
        # Act
        result = value1 % value2  # noqa (not modulo formatting)

        # Assert
        assert expected_type == type(result)
        assert expected_value == result

    @pytest.mark.parametrize(
        "value1, value2, expected",
        [
            [BaseDecimal(1, 0), BaseDecimal(2, 0), BaseDecimal(2, 0)],
            [BaseDecimal(1, 0), 2, 2],
            [BaseDecimal(1, 0), Decimal(2), Decimal(2)],
        ],
    )
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
        assert expected == result

    @pytest.mark.parametrize(
        "value1, value2, expected",
        [
            [BaseDecimal(1, 0), BaseDecimal(2, 0), BaseDecimal(1, 0)],
            [BaseDecimal(1, 0), 2, BaseDecimal(1, 0)],
            [BaseDecimal(2, 0), Decimal(1), Decimal(1)],
        ],
    )
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
        assert result == expected

    @pytest.mark.parametrize(
        "value, expected",
        [["1", 1], ["1.1", 1]],
    )
    def test_int(self, value, expected):
        # Arrange
        decimal1 = BaseDecimal(value, 1)

        # Act
        # Assert
        assert int(decimal1) == expected

    def test_hash(self):
        # Arrange
        decimal1 = BaseDecimal(1.1, 1)
        decimal2 = BaseDecimal(1.1, 1)

        # Act
        # Assert
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
        # Arrange
        # Act
        decimal_object = BaseDecimal(value, precision=precision)

        # Assert
        assert str(decimal_object) == expected

    def test_repr(self):
        # Arrange
        # Act
        result = repr(BaseDecimal(1.1, 1))

        # Assert
        assert "BaseDecimal('1.1')" == result

    @pytest.mark.parametrize(
        "value, precision, expected",
        [
            [0, 0, BaseDecimal(0, 0)],
            [-0, 0, BaseDecimal(0, 0)],
            [-1, 0, BaseDecimal(-1, 0)],
            [1, 0, BaseDecimal(1, 0)],
            [1.1, 1, BaseDecimal(1.1, 1)],
            [-1.1, 1, BaseDecimal(-1.1, 1)],
        ],
    )
    def test_as_decimal_with_various_values_returns_expected_value(
        self,
        value,
        precision,
        expected,
    ):
        # Arrange, Act
        result = BaseDecimal(value, precision=precision)

        # Assert
        assert result == expected

    @pytest.mark.parametrize(
        "value, expected",
        [[0, 0], [-0, 0], [-1, -1], [1, 1], [1.1, 1.1], [-1.1, -1.1]],
    )
    def test_as_double_with_various_values_returns_expected_value(self, value, expected):
        # Arrange
        # Act
        result = BaseDecimal(value, 1).as_double()

        # Assert
        assert result == expected


class TestPrice:
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
            ["1E-7", "1E-7", 7],
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


class TestQuantity:
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

    def test_instantiate_with_negative_value_raises_value_error(self):
        # Arrange
        # Act
        # Assert
        with pytest.raises(ValueError):
            Quantity(-1, 0)

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
        # Arrange
        # Act
        # Assert
        assert Quantity.from_str(value).to_str() == expected

    def test_str_repr(self):
        # Arrange
        quantity = Quantity(2100.1666666, 6)

        # Act
        # Assert
        assert "2100.166667" == str(quantity)
        assert "Quantity('2100.166667')" == repr(quantity)


class TestMoney:
    def test_instantiate_with_none_currency_raises_type_error(self):
        # Arrange
        # Act
        # Assert
        with pytest.raises(TypeError):
            Money(1.0, None)

    def test_instantiate_with_none_value_returns_money_with_zero_amount(self):
        # Arrange
        # Act
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
            [Decimal(), Money(0, USD)],
            [Decimal("1.1"), Money(1.1, USD)],
            [Decimal("-1.1"), Money(-1.1, USD)],
            [BaseDecimal(0, 0), Money(0, USD)],
            [BaseDecimal(1.1, 1), Money(1.1, USD)],
            [BaseDecimal(-1.1, 1), Money(-1.1, USD)],
        ],
    )
    def test_instantiate_with_various_valid_inputs_returns_expected_money(self, value, expected):
        # Arrange
        # Act
        money = Money(value, USD)

        # Assert
        assert money == expected

    @pytest.mark.parametrize(
        "value1, value2, expected1, expected2",
        [["0", -0, False, True], ["-0", 0, False, True], ["-1", -1, False, True]],
    )
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
        assert expected1 == result1
        assert expected2 == result2

    @pytest.mark.parametrize(
        "value1, value2, expected1, expected2, expected3, expected4",
        [
            [0, 0, False, False, False, False],
            [1, 0, False, False, False, False],
            [-1, 0, False, False, False, False],
        ],
    )
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
        assert expected1 == result1
        assert expected2 == result2
        assert expected3 == result3
        assert expected4 == result4

    def test_as_double_returns_expected_result(self):
        # Arrange
        # Act
        money = Money(1, USD)

        # Assert
        assert 1.0 == money.as_double()
        assert "1.00" == str(money)

    def test_initialized_with_many_decimals_rounds_to_currency_precision(self):
        # Arrange
        # Act
        result1 = Money(1000.333, USD)
        result2 = Money(5005.556666, USD)

        # Assert
        assert "1_000.33 USD" == result1.to_str()
        assert "5_005.56 USD" == result2.to_str()

    def test_hash(self):
        # Arrange
        money0 = Money(0, USD)

        # Act
        # Assert
        assert isinstance(hash(money0), int)
        assert hash(money0) == hash(money0)

    def test_str(self):
        # Arrange
        money0 = Money(0, USD)
        money1 = Money(1, USD)
        money2 = Money(1_000_000, USD)

        # Act
        # Assert
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

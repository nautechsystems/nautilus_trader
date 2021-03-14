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

import decimal
from decimal import Decimal

import pytest

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

    def test_instantiate_with_float_value_raises_type_error(self):  # User should use .from_float()
        # Arrange
        # Act
        # Assert
        with pytest.raises(TypeError):
            BaseDecimal(1.1)

    def test_instantiate_with_negative_precision_raises_value_error(self):
        # Arrange
        # Act
        # Assert
        with pytest.raises(ValueError):
            BaseDecimal(1.11, -1)

    def test_rounding_with_bogus_mode_raises_type_error(self):
        # Arrange
        # Act
        # Assert
        with pytest.raises(TypeError):
            BaseDecimal(1.11, 1, "UNKNOWN")

    @pytest.mark.parametrize(
        "value, precision, expected",
        [[BaseDecimal("2.15"), -1, Decimal("0E+1")],
         [BaseDecimal("2.15"), 0, Decimal("2")],
         [BaseDecimal("2.15"), 1, Decimal("2.2")],
         [BaseDecimal("2.255"), 2, Decimal("2.26")]],
    )
    def test_round_with_various_digits_returns_expected_decimal(self, value, precision, expected):
        # Arrange
        # Act
        result = round(value, precision)

        # Assert
        assert expected == result

    @pytest.mark.parametrize(
        "value, expected",
        [[BaseDecimal("-0"), Decimal("0")],
         [BaseDecimal("0"), Decimal("0")],
         [BaseDecimal("1"), Decimal("1")],
         [BaseDecimal("-1"), Decimal("1")],
         [BaseDecimal("-1.1"), Decimal("1.1")]],
    )
    def test_abs_with_various_values_returns_expected_decimal(self, value, expected):
        # Arrange
        # Act
        result = abs(value)

        # Assert
        assert expected == result

    @pytest.mark.parametrize(
        "value, expected",
        [[BaseDecimal("-1"), Decimal("-1")],  # Matches built-in decimal.Decimal behaviour
         [BaseDecimal("0"), Decimal("0")]],
    )
    def test_pos_with_various_values_returns_expected_decimal(self, value, expected):
        # Arrange
        # Act
        result = +value

        # Assert
        assert expected == result

    @pytest.mark.parametrize(
        "value, expected",
        [[BaseDecimal("1"), Decimal("-1")],
         [BaseDecimal("0"), Decimal("0")]],
    )
    def test_neg_with_various_values_returns_expected_decimal(self, value, expected):
        # Arrange
        # Act
        result = -value

        # Assert
        assert expected == result

    @pytest.mark.parametrize(
        "value, precision, rounding, expected",
        [[1.15, 1, decimal.ROUND_HALF_EVEN, BaseDecimal("1.1")],
         [Decimal("2.14"), 1, decimal.ROUND_UP, BaseDecimal("2.2")],
         [Decimal("2.16"), 1, decimal.ROUND_DOWN, BaseDecimal("2.1")],
         [Decimal("2.15"), 1, decimal.ROUND_HALF_UP, BaseDecimal("2.2")],
         [Decimal("2.15"), 1, decimal.ROUND_HALF_DOWN, BaseDecimal("2.1")],
         [Decimal("2.15"), 1, decimal.ROUND_HALF_EVEN, BaseDecimal("2.1")]],
    )
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
        assert expected == result

    @pytest.mark.parametrize(
        "value, expected",
        [[0, BaseDecimal()],
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
         [BaseDecimal("-1.1"), BaseDecimal("-1.1")]],
    )
    def test_instantiate_with_various_valid_inputs_returns_expected_decimal(self, value, expected):
        # Arrange
        # Act
        decimal_object = BaseDecimal(value)

        # Assert
        assert expected == decimal_object

    @pytest.mark.parametrize(
        "value, precision, expected",
        [[0., 0, BaseDecimal("0")],
         [1., 0, BaseDecimal("1")],
         [-1., 0, BaseDecimal("-1")],
         [1.123, 3, BaseDecimal("1.123")],
         [-1.123, 3, BaseDecimal("-1.123")],
         [1.155, 2, BaseDecimal("1.16")]],
    )
    def test_instantiate_with_various_precisions_returns_expected_decimal(self, value, precision, expected):
        # Arrange
        # Act
        decimal_object = BaseDecimal(value, precision)

        # Assert
        assert expected == decimal_object
        assert precision == decimal_object.precision

    @pytest.mark.parametrize(
        "value1, value2, expected",
        [["0", -0, True],
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
         ["-1.12", "-1.1", False]],
    )
    def test_equality_with_various_values_returns_expected_result(self, value1, value2, expected):
        # Arrange
        # Act
        result = BaseDecimal(value1) == BaseDecimal(value2)

        # Assert
        assert expected == result

    @pytest.mark.parametrize(
        "value1, value2, expected",
        [["0", -0, True],
         ["-0", 0, True],
         ["-1", -1, True],
         ["1", 1, True],
         ["0", 1, False],
         ["-1", 0, False],
         ["-1", -2, False],
         ["1", 2, False]],
    )
    def test_equality_with_various_int_returns_expected_result(self, value1, value2, expected):
        # Arrange
        # Act
        result1 = BaseDecimal(value1) == value2
        result2 = value2 == BaseDecimal(value1)

        # Assert
        assert expected == result1
        assert expected == result2

    @pytest.mark.parametrize(
        "value1, value2, expected",
        [[BaseDecimal(), Decimal(), True],
         [BaseDecimal("0"), Decimal(-0), True],
         [BaseDecimal("1"), Decimal(), False]],
    )
    def test_equality_with_various_decimals_returns_expected_result(self, value1, value2, expected):
        # Arrange
        # Act
        result = value1 == value2

        # Assert
        assert expected == result

    @pytest.mark.parametrize(
        "value1, value2, expected1, expected2, expected3, expected4",
        [[0, 0, False, True, True, False],
         [1, 0, True, True, False, False],
         [-1, 0, False, False, True, True],
         [BaseDecimal(0), Decimal(0), False, True, True, False],
         [BaseDecimal(1), Decimal(0), True, True, False, False],
         [BaseDecimal(-1), Decimal(0), False, False, True, True]],
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
        # Arrange
        # Act
        result1 = BaseDecimal(value1) > BaseDecimal(value2)
        result2 = BaseDecimal(value1) >= BaseDecimal(value2)
        result3 = BaseDecimal(value1) <= BaseDecimal(value2)
        result4 = BaseDecimal(value1) < BaseDecimal(value2)

        # Assert
        assert expected1 == result1
        assert expected2 == result2
        assert expected3 == result3
        assert expected4 == result4

    @pytest.mark.parametrize(
        "value1, value2, expected_type, expected_value",
        [[BaseDecimal(), BaseDecimal(), Decimal, 0],
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
         [BaseDecimal("1"), Decimal("1.1"), Decimal, Decimal("2.1")]],
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
        assert expected_type == type(result)
        assert expected_value == result

    @pytest.mark.parametrize(
        "value1, value2, expected_type, expected_value",
        [[BaseDecimal(), BaseDecimal(), Decimal, 0],
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
         [BaseDecimal("1"), Decimal("1.1"), Decimal, Decimal("-0.1")]],
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
        assert expected_type == type(result)
        assert expected_value == result

    @pytest.mark.parametrize(
        "value1, value2, expected_type, expected_value",
        [[BaseDecimal(), 0, Decimal, 0],
         [BaseDecimal(1), 1, Decimal, 1],
         [1, BaseDecimal(1), Decimal, 1],
         [2, BaseDecimal(3), Decimal, 6],
         [BaseDecimal(2), 1.0, float, 2],
         [1.1, BaseDecimal(2), float, 2.2],
         [BaseDecimal("1.1"), BaseDecimal("1.1"), Decimal, Decimal("1.21")],
         [BaseDecimal("1.1"), Decimal("1.1"), Decimal, Decimal("1.21")]],
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
        assert expected_type == type(result)
        assert expected_value == result

    @pytest.mark.parametrize(
        "value1, value2, expected_type, expected_value",
        [[1, BaseDecimal(1), Decimal, 1],
         [1.1, BaseDecimal("1.1"), float, 1],
         [BaseDecimal(), 1, Decimal, 0],
         [BaseDecimal(1), 2, Decimal, Decimal("0.5")],
         [2, BaseDecimal(1), Decimal, Decimal("2.0")],
         [BaseDecimal(2), 1.1, float, 1.8181818181818181],
         [1.1, BaseDecimal(2), float, 1.1 / 2],
         [BaseDecimal("1.1"), BaseDecimal("1.2"), Decimal, Decimal("0.9166666666666666666666666667")],
         [BaseDecimal("1.1"), Decimal("1.2"), Decimal, Decimal("0.9166666666666666666666666667")]],
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
        [[1, BaseDecimal(1), Decimal, 1],
         [BaseDecimal(), 1, Decimal, 0],
         [BaseDecimal(1), 2, Decimal, Decimal(0)],
         [2, BaseDecimal(1), Decimal, Decimal(2)],
         [2.1, BaseDecimal("1.1"), float, 1],
         [4.4, BaseDecimal("1.1"), float, 4],
         [BaseDecimal("2.1"), 1.1, float, 1],
         [BaseDecimal("4.4"), 1.1, float, 4],
         [BaseDecimal("1.1"), BaseDecimal("1.2"), Decimal, Decimal(0)],
         [BaseDecimal("1.1"), Decimal("1.2"), Decimal, Decimal(0)]],
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
        [[1, BaseDecimal(1), Decimal, 0],
         [BaseDecimal(100), 10, Decimal, 0],
         [BaseDecimal(23), 2, Decimal, 1],
         [2, BaseDecimal(1), Decimal, 0],
         [2.1, BaseDecimal("1.1"), float, 1.0],
         [1.1, BaseDecimal("2.1"), float, 1.1],
         [BaseDecimal("2.1"), 1.1, float, 1.0],
         [BaseDecimal("1.1"), 2.1, float, 1.1],
         [BaseDecimal("1.1"), BaseDecimal("0.2"), Decimal, Decimal("0.1")],
         [BaseDecimal("1.1"), Decimal("0.2"), Decimal, Decimal("0.1")]],
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
        result = value1 % value2

        # Assert
        assert expected_type == type(result)
        assert expected_value == result

    @pytest.mark.parametrize(
        "value1, value2, expected",
        [[BaseDecimal(1), BaseDecimal(2), BaseDecimal(2)],
         [BaseDecimal(1), 2, 2],
         [BaseDecimal(1), Decimal(2), Decimal(2)]],
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
        [[BaseDecimal(1), BaseDecimal(2), BaseDecimal(1)],
         [BaseDecimal(1), 2, BaseDecimal(1)],
         [BaseDecimal(2), Decimal(1), Decimal(1)]],
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
        assert expected == result

    @pytest.mark.parametrize(
        "value, expected",
        [["1", 1],
         ["1.1", 1]],
    )
    def test_int(self, value, expected):
        # Arrange
        decimal1 = BaseDecimal(value)

        # Act
        # Assert
        assert expected == int(decimal1)

    def test_hash(self):
        # Arrange
        decimal1 = BaseDecimal("1.1")
        decimal2 = BaseDecimal("1.1")

        # Act
        # Assert
        assert isinstance(hash(decimal2), int)
        assert hash(decimal1) == hash(decimal2)

    @pytest.mark.parametrize(
        "value, expected",
        [["0", "0"],
         ["-0", "-0"],
         ["-1", "-1"],
         ["1", "1"],
         ["1.1", "1.1"],
         ["-1.1", "-1.1"]],
    )
    def test_str_with_various_values_returns_expected_string(self, value, expected,):
        # Arrange
        # Act
        decimal_object = BaseDecimal(value)

        # Assert
        assert expected == str(decimal_object)

    def test_repr(self):
        # Arrange
        # Act
        result = repr(BaseDecimal("1.1"))

        # Assert
        assert "BaseDecimal('1.1')" == result

    @pytest.mark.parametrize(
        "value, expected",
        [["0", BaseDecimal()],
         ["-0", BaseDecimal()],
         ["-1", BaseDecimal("-1")],
         ["1", BaseDecimal("1")],
         ["1.1", BaseDecimal("1.1")],
         ["-1.1", BaseDecimal("-1.1")]],
    )
    def test_as_decimal_with_various_values_returns_expected_value(self, value, expected):
        # Arrange
        # Act
        result = BaseDecimal(value)

        # Assert
        assert expected == result

    @pytest.mark.parametrize(
        "value, expected",
        [["0", 0],
         ["-0", 0],
         ["-1", -1],
         ["1", 1],
         ["1.1", 1.1],
         ["-1.1", -1.1]],
    )
    def test_as_double_with_various_values_returns_expected_value(self, value, expected):
        # Arrange
        # Act
        result = BaseDecimal(value).as_double()

        # Assert
        assert expected == result


class TestPrice:

    def test_str_repr(self):
        # Arrange
        price = Price(1.00000, 5)

        # Act
        # Assert
        assert "1.00000" == str(price)
        assert "Price('1.00000')" == repr(price)


class TestQuantity:

    def test_instantiate_with_negative_value_raises_value_error(self):
        # Arrange
        # Act
        # Assert
        with pytest.raises(ValueError):
            Quantity(-1)

    @pytest.mark.parametrize(
        "value, expected",
        [[Quantity("0"), "0"],
         [Quantity("10.05"), "10.05"],
         [Quantity("1000"), "1,000"],
         [Quantity("1112"), "1,112"],
         [Quantity("120100"), "120,100"],
         [Quantity("200000"), "200,000"],
         [Quantity("1000000"), "1,000,000"],
         [Quantity("2500000"), "2,500,000"],
         [Quantity("1111111"), "1,111,111"],
         [Quantity("2523000"), "2,523,000"],
         [Quantity("100000000"), "100,000,000"]],
    )
    def test_str_and_to_str(self, value, expected):
        # Arrange
        # Act
        # Assert
        assert expected == Quantity(value).to_str()

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
        [[0, Money(0, USD)],
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
         [BaseDecimal("-1.1"), Money("-1.1", USD)]],
    )
    def test_instantiate_with_various_valid_inputs_returns_expected_money(self, value, expected):
        # Arrange
        # Act
        money = Money(value, USD)

        # Assert
        assert expected == money

    @pytest.mark.parametrize(
        "value1, value2, expected1, expected2",
        [["0", -0, False, True],
         ["-0", 0, False, True],
         ["-1", -1, False, True]],
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
        [[0, 0, False, False, False, False],
         [1, 0, False, False, False, False],
         [-1, 0, False, False, False, False]],
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

    def test_from_str_with_no_decimal(self):
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
        assert "1,000.33 USD" == result1.to_str()
        assert "5,005.56 USD" == result2.to_str()

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
        assert "1,000,000.00 USD" == money2.to_str()

    def test_repr(self):
        # Arrange
        money = Money("1.00", USD)

        # Act
        result = repr(money)

        # Assert
        assert "Money('1.00', USD)" == result

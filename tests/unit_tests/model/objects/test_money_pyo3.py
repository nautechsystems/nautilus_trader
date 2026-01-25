# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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
from typing import Any

import pytest

from nautilus_trader.core.nautilus_pyo3 import Currency
from nautilus_trader.core.nautilus_pyo3 import Money
from nautilus_trader.model import convert_to_raw_int
from nautilus_trader.model.objects import MONEY_MAX
from nautilus_trader.model.objects import MONEY_MIN


AUD = Currency.from_str("AUD")
USD = Currency.from_str("USD")
USDT = Currency.from_str("USDT")


class TestMoney:
    def test_instantiate_with_nan_raises_value_error(self):
        # Arrange, Act, Assert
        with pytest.raises(ValueError):
            Money(math.nan, currency=USD)

    def test_instantiate_with_none_value_raises_type_error(self) -> None:
        # Arrange, Act, Assert
        with pytest.raises(TypeError):
            Money(None, currency=USD)  # type: ignore

    def test_instantiate_with_none_currency_raises_type_error(self) -> None:
        # Arrange, Act, Assert
        with pytest.raises(TypeError):
            Money(1.0, None)  # type: ignore

    def test_instantiate_with_value_exceeding_positive_limit_raises_value_error(self) -> None:
        # Arrange, Act, Assert
        with pytest.raises(ValueError):
            Money(MONEY_MAX + 1, currency=USD)

    def test_instantiate_with_value_exceeding_negative_limit_raises_value_error(self) -> None:
        # Arrange, Act, Assert
        with pytest.raises(ValueError):
            Money(MONEY_MIN - 1, currency=USD)

    @pytest.mark.parametrize(
        ("value", "expected"),
        [
            [0, Money(0, USD)],
            [1, Money(1, USD)],
            [-1, Money(-1, USD)],
        ],
    )
    def test_instantiate_with_various_valid_inputs_returns_expected_money(
        self,
        value: Any,
        expected: Money,
    ) -> None:
        # Arrange, Act
        money = Money(value, USD)

        # Assert
        assert money == expected

    def test_pickling(self):
        # Arrange
        money = Money(1, USD)

        # Act
        pickled = pickle.dumps(money)
        unpickled = pickle.loads(pickled)  # noqa: S301 (pickle is safe here)

        # Assert
        assert unpickled == money

    def test_as_double_returns_expected_result(self) -> None:
        # Arrange, Act
        amount = 1.0
        money = Money(amount, USD)

        # Assert
        assert money.as_double() == amount
        assert money.raw == convert_to_raw_int(amount, USD.precision)
        assert str(money) == "1.00 USD"

    def test_initialized_with_many_decimals_rounds_to_currency_precision(self) -> None:
        # Arrange, Act
        amount1 = 1000.333
        amount2 = 5005.556666
        result1 = Money(amount1, USD)
        result2 = Money(amount2, USD)

        # Assert
        assert result1.raw == convert_to_raw_int(amount1, USD.precision)
        assert result2.raw == convert_to_raw_int(amount2, USD.precision)
        assert str(result1) == "1000.33 USD"
        assert str(result2) == "5005.56 USD"
        assert result1.to_formatted_str() == "1_000.33 USD"
        assert result2.to_formatted_str() == "5_005.56 USD"

    def test_equality_with_different_currencies_raises_value_error(self) -> None:
        # Arrange
        money1 = Money(1, USD)
        money2 = Money(1, AUD)

        # Act, Assert
        with pytest.raises(ValueError):
            assert money1 != money2

    def test_equality(self) -> None:
        # Arrange
        money1 = Money(1, USD)
        money2 = Money(1, USD)
        money3 = Money(2, USD)

        # Act, Assert
        assert money1 == money2
        assert money1 != money3

    def test_hash(self) -> None:
        # Arrange
        money0 = Money(0, USD)

        # Act, Assert
        assert isinstance(hash(money0), int)
        assert hash(money0) == hash(money0)

    def test_str(self) -> None:
        # Arrange
        money0 = Money(0, USD)
        money1 = Money(1, USD)
        money2 = Money(1_000_000, USD)

        # Act, Assert
        assert str(money0) == "0.00 USD"
        assert str(money1) == "1.00 USD"
        assert str(money2) == "1000000.00 USD"
        assert money2.to_formatted_str() == "1_000_000.00 USD"

    def test_repr(self) -> None:
        # Arrange
        money = Money(1.00, USD)

        # Act
        result = repr(money)

        # Assert
        assert result == "Money(1.00, USD)"

    @pytest.mark.parametrize(
        ("value", "currency", "expected"),
        [
            [0, USDT, Money(0, USDT)],
            [convert_to_raw_int(1, USD.precision), USD, Money(1.00, USD)],
            [convert_to_raw_int(10, AUD.precision), AUD, Money(10.00, AUD)],
        ],
    )
    def test_from_raw_given_valid_values_returns_expected_result(
        self,
        value: int,
        currency: Currency,
        expected: Money,
    ) -> None:
        # Arrange, Act
        result = Money.from_raw(value, currency)

        # Assert
        assert result == expected

    def test_from_str_when_malformed_raises_value_error(self) -> None:
        # Arrange
        value = "@"

        # Act, Assert
        with pytest.raises(ValueError):
            Money.from_str(value)

    @pytest.mark.parametrize(
        ("value", "expected"),
        [
            ["1.00 USDT", Money(1.00, USDT)],
            ["1.00 USD", Money(1.00, USD)],
            ["1.001 AUD", Money(1.00, AUD)],
        ],
    )
    def test_from_str_given_valid_strings_returns_expected_result(
        self,
        value: str,
        expected: Money,
    ) -> None:
        # Arrange, Act
        result1 = Money.from_str(value)
        result2 = Money.from_str(value)

        # Assert
        assert result1 == result2
        assert result1 == expected


class TestMoneyArithmetic:
    """
    Comprehensive arithmetic tests for PyO3 Money type.

    PyO3 Money arithmetic returns Money for Money-Money operations (same currency).

    """

    @pytest.mark.parametrize(
        ("value1", "value2", "expected_type", "expected_value"),
        [
            # Money + Money → Money
            [Money(1.00, USD), Money(2.00, USD), "Money", Money(3.00, USD)],
            [Money(0.00, USD), Money(0.00, USD), "Money", Money(0.00, USD)],
            [Money(-1.00, USD), Money(1.00, USD), "Money", Money(0.00, USD)],
            [Money(100.50, USD), Money(50.25, USD), "Money", Money(150.75, USD)],
            # Money + int → Decimal
            [Money(1.00, USD), 2, "Decimal", "3.00"],
            [Money(0.00, USD), 0, "Decimal", "0.00"],
            [Money(-1.00, USD), 1, "Decimal", "0.00"],
            # int + Money → Decimal
            [2, Money(1.00, USD), "Decimal", "3.00"],
            [0, Money(0.00, USD), "Decimal", "0.00"],
            [1, Money(-1.00, USD), "Decimal", "0.00"],
            # Money + float → float
            [Money(1.00, USD), 2.5, "float", 3.5],
            [Money(0.00, USD), 0.0, "float", 0.0],
            [Money(-1.00, USD), 1.5, "float", 0.5],
            # float + Money → float
            [2.5, Money(1.00, USD), "float", 3.5],
            [0.0, Money(0.00, USD), "float", 0.0],
            [1.5, Money(-1.00, USD), "float", 0.5],
        ],
    )
    def test_addition_with_various_types(
        self,
        value1: Any,
        value2: Any,
        expected_type: str,
        expected_value: Any,
    ) -> None:
        # Arrange
        from decimal import Decimal

        # Act
        result = value1 + value2

        # Assert
        if expected_type == "Money":
            assert isinstance(result, Money)
            assert result == expected_value
        elif expected_type == "Decimal":
            assert isinstance(result, Decimal)
            assert result == Decimal(expected_value)
        elif expected_type == "float":
            assert isinstance(result, float)
            assert result == expected_value

    def test_addition_with_different_currencies_raises_value_error(self) -> None:
        # Arrange
        money1 = Money(1.00, USD)
        money2 = Money(1.00, AUD)

        # Act, Assert
        with pytest.raises(ValueError, match="Currency mismatch"):
            _ = money1 + money2  # type: ignore[operator]

    @pytest.mark.parametrize(
        ("value1", "value2", "expected_type", "expected_value"),
        [
            # Money - Money → Money
            [Money(3.00, USD), Money(2.00, USD), "Money", Money(1.00, USD)],
            [Money(0.00, USD), Money(0.00, USD), "Money", Money(0.00, USD)],
            [Money(1.00, USD), Money(1.00, USD), "Money", Money(0.00, USD)],
            [Money(100.50, USD), Money(50.25, USD), "Money", Money(50.25, USD)],
            # Money - int → Decimal
            [Money(3.00, USD), 2, "Decimal", "1.00"],
            [Money(0.00, USD), 0, "Decimal", "0.00"],
            [Money(1.00, USD), 1, "Decimal", "0.00"],
            # int - Money → Decimal
            [3, Money(2.00, USD), "Decimal", "1.00"],
            [0, Money(0.00, USD), "Decimal", "0.00"],
            [1, Money(1.00, USD), "Decimal", "0.00"],
            # Money - float → float
            [Money(3.00, USD), 2.5, "float", 0.5],
            [Money(0.00, USD), 0.0, "float", 0.0],
            [Money(1.00, USD), 0.5, "float", 0.5],
            # float - Money → float
            [3.5, Money(2.00, USD), "float", 1.5],
            [0.0, Money(0.00, USD), "float", 0.0],
            [1.5, Money(1.00, USD), "float", 0.5],
        ],
    )
    def test_subtraction_with_various_types(
        self,
        value1: Any,
        value2: Any,
        expected_type: str,
        expected_value: Any,
    ) -> None:
        # Arrange
        from decimal import Decimal

        # Act
        result = value1 - value2

        # Assert
        if expected_type == "Money":
            assert isinstance(result, Money)
            assert result == expected_value
        elif expected_type == "Decimal":
            assert isinstance(result, Decimal)
            assert result == Decimal(expected_value)
        elif expected_type == "float":
            assert isinstance(result, float)
            assert result == expected_value

    def test_subtraction_with_different_currencies_raises_value_error(self) -> None:
        # Arrange
        money1 = Money(1.00, USD)
        money2 = Money(1.00, AUD)

        # Act, Assert
        with pytest.raises(ValueError, match="Currency mismatch"):
            _ = money1 - money2  # type: ignore[operator]

    @pytest.mark.parametrize(
        ("value1", "value2", "expected_type", "expected_value"),
        [
            # Money * int → Decimal
            [Money(2.00, USD), 3, "Decimal", "6.00"],
            [Money(0.00, USD), 5, "Decimal", "0.00"],
            [Money(-2.00, USD), 3, "Decimal", "-6.00"],
            # int * Money → Decimal
            [3, Money(2.00, USD), "Decimal", "6.00"],
            [5, Money(0.00, USD), "Decimal", "0.00"],
            [3, Money(-2.00, USD), "Decimal", "-6.00"],
            # Money * float → float
            [Money(2.00, USD), 1.5, "float", 3.0],
            [Money(0.00, USD), 5.0, "float", 0.0],
            [Money(-2.00, USD), 1.5, "float", -3.0],
            # float * Money → float
            [1.5, Money(2.00, USD), "float", 3.0],
            [5.0, Money(0.00, USD), "float", 0.0],
            [1.5, Money(-2.00, USD), "float", -3.0],
            # Money * Money → Decimal
            [Money(2.00, USD), Money(3.00, USD), "Decimal", "6.00"],
            [Money(1.50, USD), Money(2.00, USD), "Decimal", "3.00"],
        ],
    )
    def test_multiplication_with_various_types(
        self,
        value1: Any,
        value2: Any,
        expected_type: str,
        expected_value: Any,
    ) -> None:
        # Arrange
        from decimal import Decimal

        # Act
        result = value1 * value2

        # Assert
        if expected_type == "Decimal":
            assert isinstance(result, Decimal)
            assert result == Decimal(expected_value)
        elif expected_type == "float":
            assert isinstance(result, float)
            assert result == expected_value

    @pytest.mark.parametrize(
        ("value1", "value2", "expected_type", "expected_value"),
        [
            # Money / int → Decimal
            [Money(6.00, USD), 3, "Decimal", "2.00"],
            [Money(0.00, USD), 5, "Decimal", "0.00"],
            [Money(-6.00, USD), 3, "Decimal", "-2.00"],
            # int / Money → Decimal
            [6, Money(3.00, USD), "Decimal", "2.00"],
            [0, Money(5.00, USD), "Decimal", "0.00"],
            # Money / float → float
            [Money(6.00, USD), 2.0, "float", 3.0],
            [Money(0.00, USD), 5.0, "float", 0.0],
            [Money(-6.00, USD), 2.0, "float", -3.0],
            # float / Money → float
            [6.0, Money(2.00, USD), "float", 3.0],
            [0.0, Money(5.00, USD), "float", 0.0],
            # Money / Money → Decimal
            [Money(6.00, USD), Money(3.00, USD), "Decimal", "2.00"],
            [Money(10.00, USD), Money(4.00, USD), "Decimal", "2.50"],
        ],
    )
    def test_division_with_various_types(
        self,
        value1: Any,
        value2: Any,
        expected_type: str,
        expected_value: Any,
    ) -> None:
        # Arrange
        from decimal import Decimal

        # Act
        result = value1 / value2

        # Assert
        if expected_type == "Decimal":
            assert isinstance(result, Decimal)
            assert result == Decimal(expected_value)
        elif expected_type == "float":
            assert isinstance(result, float)
            assert result == expected_value

    @pytest.mark.parametrize(
        ("value1", "value2", "expected_type", "expected_value"),
        [
            # Money // int → Decimal
            [Money(7.00, USD), 3, "Decimal", "2"],
            [Money(10.00, USD), 3, "Decimal", "3"],
            # int // Money → Decimal
            [7, Money(3.00, USD), "Decimal", "2"],
            [10, Money(3.00, USD), "Decimal", "3"],
            # Money // float → float
            [Money(7.00, USD), 3.0, "float", 2.0],
            [Money(10.00, USD), 3.0, "float", 3.0],
            # float // Money → float
            [7.0, Money(3.00, USD), "float", 2.0],
            [10.0, Money(3.00, USD), "float", 3.0],
            # Money // Money → Decimal
            [Money(7.00, USD), Money(3.00, USD), "Decimal", "2"],
            [Money(10.00, USD), Money(3.00, USD), "Decimal", "3"],
        ],
    )
    def test_floor_division_with_various_types(
        self,
        value1: Any,
        value2: Any,
        expected_type: str,
        expected_value: Any,
    ) -> None:
        # Arrange
        from decimal import Decimal

        # Act
        result = value1 // value2

        # Assert
        if expected_type == "Decimal":
            assert isinstance(result, Decimal)
            assert result == Decimal(expected_value)
        elif expected_type == "float":
            assert isinstance(result, float)
            assert result == expected_value

    @pytest.mark.parametrize(
        ("value1", "value2", "expected_type", "expected_value"),
        [
            # Money % int → Decimal
            [Money(7.00, USD), 3, "Decimal", "1.00"],
            [Money(10.00, USD), 3, "Decimal", "1.00"],
            # Money % float → float
            [Money(7.00, USD), 3.0, "float", 1.0],
            [Money(10.00, USD), 3.0, "float", 1.0],
            # Money % Money → Decimal
            [Money(7.00, USD), Money(3.00, USD), "Decimal", "1.00"],
            [Money(10.00, USD), Money(3.00, USD), "Decimal", "1.00"],
        ],
    )
    def test_mod_with_various_types(
        self,
        value1: Any,
        value2: Any,
        expected_type: str,
        expected_value: Any,
    ) -> None:
        # Arrange
        from decimal import Decimal

        # Act
        result = value1 % value2

        # Assert
        if expected_type == "Decimal":
            assert isinstance(result, Decimal)
            assert result == Decimal(expected_value)
        elif expected_type == "float":
            assert isinstance(result, float)
            assert result == expected_value

    @pytest.mark.parametrize(
        ("value", "expected"),
        [
            [Money(1.00, USD), "-1.00"],
            [Money(-1.00, USD), "1.00"],
            [Money(0.00, USD), "0.00"],
        ],
    )
    def test_negation(self, value: Money, expected: str) -> None:
        # Arrange
        from decimal import Decimal

        # Act
        result = -value  # type: ignore[operator]

        # Assert
        assert isinstance(result, Decimal)
        assert result == Decimal(expected)

    @pytest.mark.parametrize(
        ("value", "expected"),
        [
            [Money(1.00, USD), "1.00"],
            [Money(-1.00, USD), "1.00"],
            [Money(0.00, USD), "0.00"],
        ],
    )
    def test_abs(self, value: Money, expected: str) -> None:
        # Arrange
        from decimal import Decimal

        # Act
        result = abs(value)  # type: ignore[arg-type, var-annotated]

        # Assert
        assert isinstance(result, Decimal)
        assert result == Decimal(expected)

    @pytest.mark.parametrize(
        ("value", "expected"),
        [
            [Money(1.00, USD), "1.00"],
            [Money(-1.00, USD), "1.00"],  # PyO3 returns absolute value
            [Money(0.00, USD), "0.00"],
        ],
    )
    def test_pos(self, value: Money, expected: str) -> None:
        # Arrange
        from decimal import Decimal

        # Act
        result = +value  # type: ignore[operator]

        # Assert
        assert isinstance(result, Decimal)
        assert result == Decimal(expected)
